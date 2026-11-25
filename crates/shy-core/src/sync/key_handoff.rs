//! Getting the storage settings from another member (#259). A device that
//! was offline while the admin changed the bucket key, and stayed away until
//! the old key was deleted, can't read the bucket to learn the new one. When
//! the bucket refuses its key, it asks members one at a time; each answers
//! from its own store with the admin's latest settings op and the admin's
//! commits needed to open it. The asker checks both as if they came from the
//! bucket, so a member can withhold or delay settings but never forge them.

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use iroh::PublicKey;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use super::receive::{AuthorKey, author_epoch_key};
use crate::{
    config, crypto,
    membership::follow::CommitOutcome,
    ops::{Envelope, Op},
    storage::Store,
    transport::{gossip::Net, heads::is_refused, key_handoff::KeyRequestIn},
    views::{Change, PresenceState, member_id},
    workspace::Workspace,
};

/// The most admin commits an answer carries. A device further behind than
/// this needs a new invite.
pub const MAX_HANDOFF_COMMITS: usize = 64;
/// The most bytes of commits an answer carries.
pub const MAX_HANDOFF_COMMIT_BYTES: usize = 1024 * 1024;
/// Members asked per sync pass, so a pass never waits on everyone.
const ASKS_PER_PASS: usize = 4;
/// After asking everyone, how long before asking them all again.
const ROUND_EVERY: Duration = Duration::from_secs(10 * 60);
/// How often one member is answered.
const ANSWER_EVERY: Duration = Duration::from_secs(60);

/// What the asker has: its epoch and when its settings were last changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyRequest {
    pub epoch: u64,
    pub settings_lamport: u64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyAnswer {
    /// The admin's commits from the asker's epoch (by the epoch each moves
    /// on from), then the admin's latest settings op as it was sealed.
    Settings {
        commits: Vec<(u64, Vec<u8>)>,
        settings: Envelope,
    },
    /// Nothing newer than what the asker has, or nothing this member can
    /// hand over (it lacks the commits or the op).
    NothingNewer,
    /// The asker is more commits behind than an answer carries.
    TooFarBehind,
}

/// Sizes only: the contents are sealed, but they needn't be in a log either.
impl std::fmt::Debug for KeyAnswer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Settings { commits, settings } => formatter
                .debug_struct("Settings")
                .field("commits", &commits.len())
                .field("settings_epoch", &settings.epoch)
                .finish_non_exhaustive(),
            Self::NothingNewer => formatter.write_str("NothingNewer"),
            Self::TooFarBehind => formatter.write_str("TooFarBehind"),
        }
    }
}

/// The asking side's round since its key stopped working: who answered
/// (not asked again this round) and when each member was last tried, so
/// members who couldn't be reached take turns.
#[derive(Debug, Default)]
pub struct KeyRound {
    answered: HashSet<PublicKey>,
    last_tried: HashMap<PublicKey, tokio::time::Instant>,
    started: Option<tokio::time::Instant>,
    /// Members who said we're too far behind; believed only after a full
    /// round, and if every member who answered said so.
    too_far_behind: HashSet<PublicKey>,
}

/// Where a device stands with its bucket key after trying to recover it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyState {
    /// The bucket takes it, or refused for another reason than the key.
    Working,
    /// It works again, with settings another member handed over.
    Recovered,
    /// Refused, and nobody asked so far could help.
    Refused,
    /// Refused, and members said this device is too far behind.
    TooFarBehind,
}

/// Asks one member for the storage settings.
pub(crate) trait KeyAsker {
    async fn ask(&self, member: PublicKey, request: KeyRequest) -> Result<KeyAnswer>;
}

impl Workspace {
    /// The answer for `asker`, a current member whose key stopped working.
    /// Fails for anyone else: membership is read now, not when they
    /// connected, so someone removed since gets nothing.
    pub(crate) fn key_answer_for(&self, asker: &PublicKey, request: &KeyRequest) -> Result<KeyAnswer> {
        self.key_answer_within(asker, request, MAX_HANDOFF_COMMITS, MAX_HANDOFF_COMMIT_BYTES)
    }

    pub(crate) fn key_answer_within(
        &self,
        asker: &PublicKey,
        request: &KeyRequest,
        max_commits: usize,
        max_bytes: usize,
    ) -> Result<KeyAnswer> {
        self.ensure_answerable(asker)?;
        let answer = self.answer_from_store(request, max_commits, max_bytes)?;
        // Again: a commit removing the asker may have applied meanwhile.
        self.ensure_answerable(asker)?;
        Ok(answer)
    }

    fn ensure_answerable(&self, asker: &PublicKey) -> Result<()> {
        ensure!(
            *asker != self.me() && self.current_members()?.contains(asker),
            "only a current member is handed the storage settings"
        );
        Ok(())
    }

    fn answer_from_store(
        &self,
        request: &KeyRequest,
        max_commits: usize,
        max_bytes: usize,
    ) -> Result<KeyAnswer> {
        let state = self.state();
        let lamport = config::bucket_lamport(&state.store)?;
        if lamport <= request.settings_lamport {
            return Ok(KeyAnswer::NothingNewer);
        }
        let Some(settings) = settings_envelope(&state.store, &self.inner.config.admin, lamport)? else {
            return Ok(KeyAnswer::NothingNewer);
        };
        let needed = settings.epoch.saturating_sub(request.epoch);
        if needed > max_commits as u64 {
            return Ok(KeyAnswer::TooFarBehind);
        }
        let commits = state
            .store
            .commit_records_between(request.epoch, settings.epoch)?;
        if commits.len() as u64 != needed {
            return Ok(KeyAnswer::NothingNewer);
        }
        if commits.iter().map(|(_, bytes)| bytes.len()).sum::<usize>() > max_bytes {
            return Ok(KeyAnswer::TooFarBehind);
        }
        Ok(KeyAnswer::Settings { commits, settings })
    }

    /// Takes what a member handed over: the admin's commits first, each
    /// checked as if read from the bucket, then their settings op, applied
    /// only if newer than ours. Returns whether the settings changed.
    pub(crate) fn take_key_answer(&self, answer: KeyAnswer) -> Result<bool> {
        let KeyAnswer::Settings {
            mut commits,
            settings,
        } = answer
        else {
            return Ok(false);
        };
        ensure!(
            settings.author == self.inner.config.admin,
            "storage settings from someone other than the admin"
        );
        commits.sort_by_key(|(from_epoch, _)| *from_epoch);
        for (from_epoch, bytes) in commits {
            let epoch = self.state().mls.epoch()?;
            if from_epoch < epoch {
                continue;
            }
            ensure!(
                from_epoch == epoch,
                "a commit is missing before epoch {from_epoch}"
            );
            if self.apply_admin_commit(from_epoch, &bytes)? == CommitOutcome::RemovedSelf {
                return Ok(false);
            }
        }
        let verified = {
            let state = self.state();
            match author_epoch_key(&state, &settings)? {
                AuthorKey::Ready(key) => crypto::open(&settings, &key)?,
                AuthorKey::NotYet => bail!("the storage settings are sealed for an epoch we haven't reached"),
                AuthorKey::Refused(reason) => bail!(reason),
            }
        };
        let Op::StorageSettings { bucket } = &verified.body.op else {
            bail!("handed an op that isn't storage settings");
        };
        let is_changed = self.state().store.atomically(|store| {
            config::check_settings_change(store, bucket)?;
            let is_changed = config::update_bucket(store, bucket, verified.body.lamport)?;
            // The op itself arrives through sync, and is recorded then.
            if is_changed {
                config::record_settings_op(store, config::NO_SETTINGS_OP)?;
            }
            Ok(is_changed)
        })?;
        if is_changed {
            self.emit(Change::Storage);
        }
        Ok(is_changed)
    }

    /// When the bucket refuses our key, asks members who haven't been asked
    /// this round for the current settings, until one works.
    pub(crate) async fn recover_storage_key(&self, asker: &impl KeyAsker) -> Result<KeyState> {
        if self.is_admin() || self.status().removed || self.is_key_working().await {
            self.key_round().take();
            return Ok(KeyState::Working);
        }
        let mut asked_this_pass = HashSet::new();
        for _ in 0..ASKS_PER_PASS {
            let Some((member, is_retry_while_offline)) = self.next_member_to_ask(&asked_this_pass)? else {
                break;
            };
            asked_this_pass.insert(member);
            let request = self.key_request()?;
            let answer = match asker.ask(member, request).await {
                Ok(answer) => {
                    self.key_round().answered.insert(member);
                    answer
                }
                Err(error) => {
                    debug!(
                        "{} couldn't hand over the storage settings: {error:#}",
                        member_id(&member)
                    );
                    // Everyone gets one try a round; after that, members who
                    // seem offline are retried one a pass, as each can take
                    // the whole timeout.
                    match is_retry_while_offline {
                        true => break,
                        false => continue,
                    }
                }
            };
            if answer == KeyAnswer::TooFarBehind {
                self.key_round().too_far_behind.insert(member);
                continue;
            }
            match self.take_key_answer(answer) {
                Ok(true) if self.is_key_working().await => {
                    self.key_round().take();
                    return Ok(KeyState::Recovered);
                }
                Ok(_) => {}
                Err(error) => warn!("ignoring storage settings from {}: {error:#}", member_id(&member)),
            }
            if self.status().removed {
                return Ok(KeyState::Working);
            }
        }
        let me = self.me();
        let others: Vec<PublicKey> = self
            .current_members()?
            .into_iter()
            .filter(|member| *member != me)
            .collect();
        Ok(match self.key_round().says_too_far_behind(&others) {
            true => KeyState::TooFarBehind,
            false => KeyState::Refused,
        })
    }

    pub(crate) fn key_request(&self) -> Result<KeyRequest> {
        let state = self.state();
        Ok(KeyRequest {
            epoch: state.mls.epoch()?,
            settings_lamport: config::bucket_lamport(&state.store)?,
        })
    }

    /// Whether the bucket takes our key; only a refusal (401 or 403) says
    /// it doesn't, never a network error.
    async fn is_key_working(&self) -> bool {
        match self.bucket().probe_access(&self.me()).await {
            Err(error) => !is_refused(&error),
            Ok(()) => true,
        }
    }

    /// The next member to ask: one who hasn't answered this round nor been
    /// asked this pass, those online first, then whoever was tried longest
    /// ago. Also says whether it is a retry of one who seems offline. A
    /// round ends after a while, and then everyone may be asked again.
    fn next_member_to_ask(&self, asked_this_pass: &HashSet<PublicKey>) -> Result<Option<(PublicKey, bool)>> {
        let me = self.me();
        let now = crate::ops::now_ms()?;
        let members: Vec<(PublicKey, bool)> = self
            .current_members()?
            .into_iter()
            .filter(|member| *member != me)
            .map(|member| (member, self.presence_of(&member, now) == PresenceState::Online))
            .collect();
        let mut round = self.key_round();
        let started = *round.started.get_or_insert_with(tokio::time::Instant::now);
        if started.elapsed() >= ROUND_EVERY {
            *round = KeyRound::default();
            round.started = Some(tokio::time::Instant::now());
        }
        let next = members
            .into_iter()
            .filter(|(member, _)| !round.answered.contains(member) && !asked_this_pass.contains(member))
            .min_by_key(|(member, is_online)| (!is_online, round.last_tried.get(member).copied()));
        Ok(next.map(|(member, is_online)| {
            let was_tried = round
                .last_tried
                .insert(member, tokio::time::Instant::now())
                .is_some();
            (member, was_tried && !is_online)
        }))
    }

    /// Answers a member's request off the async threads, as it reads the
    /// store.
    pub(crate) fn answer_key_request(&self, asked: KeyRequestIn) {
        if !self.may_answer_key_request(&asked.from) {
            let _ = asked.reply.send(None);
            return;
        }
        let workspace = self.clone();
        self.inner.runtime.spawn_blocking(move || {
            let answer = match workspace.key_answer_for(&asked.from, &asked.request) {
                Ok(answer) => Some(answer),
                Err(error) => {
                    debug!(
                        "not handing the storage settings to {}: {error:#}",
                        member_id(&asked.from)
                    );
                    None
                }
            };
            let _ = asked.reply.send(answer);
        });
    }

    /// Whether `asker` may be answered now: once a minute per member, so
    /// nobody can keep us reading our store for them.
    pub(crate) fn may_answer_key_request(&self, asker: &PublicKey) -> bool {
        let mut answers = self
            .inner
            .key_answers
            .lock()
            .expect("key answers lock poisoned by an earlier panic");
        let now = tokio::time::Instant::now();
        answers.retain(|_, last| now.duration_since(*last) < ANSWER_EVERY);
        if answers.contains_key(asker) {
            return false;
        }
        answers.insert(*asker, now);
        true
    }

    fn key_round(&self) -> std::sync::MutexGuard<'_, KeyRound> {
        self.inner
            .key_round
            .lock()
            .expect("key round lock poisoned by an earlier panic")
    }
}

impl KeyAsker for Net {
    async fn ask(&self, member: PublicKey, request: KeyRequest) -> Result<KeyAnswer> {
        self.ask_for_storage_key(member, &request).await
    }
}

impl KeyRound {
    fn take(&mut self) {
        *self = Self::default();
    }

    /// Whether to tell the person they need a new invite: only once every
    /// other member was tried this round, and every one who answered said
    /// we're too far behind, so no single member can make them give up.
    fn says_too_far_behind(&self, others: &[PublicKey]) -> bool {
        let is_full_round = others.iter().all(|member| self.last_tried.contains_key(member));
        is_full_round && !self.too_far_behind.is_empty() && self.too_far_behind == self.answered
    }
}

/// The admin's op that set the settings in use (the one with their
/// `lamport`), if this device applied it; settings that came with an invite
/// or from another member have none until the op itself arrives.
fn settings_envelope(store: &Store, admin: &PublicKey, lamport: u64) -> Result<Option<Envelope>> {
    let seq = match config::settings_op_seq(store)? {
        Some(seq) => seq,
        // Applied by a version that kept no record: found once, then recorded.
        None => {
            let found = find_settings_op(store, admin, lamport)?.unwrap_or(config::NO_SETTINGS_OP);
            config::record_settings_op(store, found)?;
            found
        }
    };
    if seq == config::NO_SETTINGS_OP {
        return Ok(None);
    }
    let Some(envelope) = store.envelopes_between(admin, seq, seq)?.pop() else {
        return Ok(None);
    };
    let Some(key) = store.epoch_key(envelope.epoch)? else {
        return Ok(None);
    };
    let verified = crypto::open(&envelope, &key).context("opening our copy of the storage settings")?;
    let is_those_settings =
        matches!(verified.body.op, Op::StorageSettings { .. }) && verified.body.lamport == lamport;
    Ok(is_those_settings.then_some(envelope))
}

/// The admin's ops read at a time while looking for their settings op.
const SEARCH_BATCH: u64 = 256;

/// The seq of the admin's settings op with `lamport`, read newest first.
/// An author's clock only rises along their log, so the search stops at
/// the first op older than that.
fn find_settings_op(store: &Store, admin: &PublicKey, lamport: u64) -> Result<Option<u64>> {
    let mut last = store.last_seq(admin)?;
    while last > 0 {
        let first = last.saturating_sub(SEARCH_BATCH - 1).max(1);
        for envelope in store.envelopes_between(admin, first, last)?.into_iter().rev() {
            let Some(key) = store.epoch_key(envelope.epoch)? else {
                continue;
            };
            let Ok(verified) = crypto::open(&envelope, &key) else {
                continue;
            };
            if verified.body.lamport < lamport {
                return Ok(None);
            }
            if verified.body.lamport == lamport && matches!(verified.body.op, Op::StorageSettings { .. }) {
                return Ok(Some(envelope.seq));
            }
        }
        last = first - 1;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;

    #[test]
    fn one_answer_isnt_believed_before_everyone_was_tried() {
        let [bob, carol] = [(); 2].map(|()| SecretKey::generate().public());
        let mut round = KeyRound::default();
        let now = tokio::time::Instant::now();
        round.last_tried.insert(bob, now);
        round.answered.insert(bob);
        round.too_far_behind.insert(bob);
        assert!(
            !round.says_too_far_behind(&[bob, carol]),
            "Carol wasn't tried yet"
        );

        round.last_tried.insert(carol, now);
        assert!(
            round.says_too_far_behind(&[bob, carol]),
            "Carol was tried and didn't answer"
        );

        round.answered.insert(carol);
        assert!(!round.says_too_far_behind(&[bob, carol]), "Carol disagreed");
    }
}
