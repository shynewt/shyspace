//! Finding out whose log moved without listing everyone's segments: after
//! each upload an author rewrites their head (`transport/heads.rs`) and
//! gossips a signed hint naming its version. A check lists `heads/` once and
//! reads only the heads that changed in a way a hint didn't already explain.

use std::{collections::HashSet, sync::atomic::Ordering};

use anyhow::{Context, Result, ensure};
use iroh::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{transport::heads::Head, workspace::Workspace};

const SIGNATURE_DOMAIN: &str = "shyspace head hint v1";
/// ETags are short; a longer version is junk.
const MAX_VERSION_LENGTH: usize = 256;

/// "My head is at `seq`, version `head_version`", signed by its author for
/// this workspace only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hint {
    pub author: PublicKey,
    pub seq: u64,
    pub head_version: String,
    pub signature: Vec<u8>,
}

fn signed_bytes(workspace_id: &str, author: &PublicKey, seq: u64, head_version: &str) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(
        SIGNATURE_DOMAIN,
        workspace_id,
        author.as_bytes(),
        seq,
        head_version,
    ))?)
}

impl Workspace {
    /// Writes our head after an upload and tells online peers its version.
    /// A head ahead of what we hold may be right (a restored device that
    /// hasn't read its own log yet), so we read our log first; if it still
    /// claims more than we hold, someone else wrote it, and we correct it.
    pub(crate) async fn write_head(&self) -> Result<()> {
        let me = self.me();
        if let Some(current) = self.bucket().head(&me).await?
            && current.last_seq > self.state().store.last_seq(&me)?
            && let Some(error) = self.pull_author(&me).await?
        {
            return Err(error.context("reading our own log before writing our head"));
        }
        let pushed = self.state().store.last_pushed_seq(&me)?;
        if pushed == 0 {
            return Ok(());
        }
        if self.bucket().head(&me).await? == Some(Head { last_seq: pushed }) {
            self.inner.confirmed_head_seq.store(pushed, Ordering::SeqCst);
            return Ok(());
        }
        let version = self.bucket().put_head(&me, Head { last_seq: pushed }).await?;
        self.note_seen_head(&me, &version);
        self.inner.confirmed_head_seq.store(pushed, Ordering::SeqCst);
        if let Some(net) = &self.inner.net {
            let sent = self.signed_hint(pushed, version);
            if let Err(error) = async { net.broadcast_hint(sent?).await }.await {
                warn!("couldn't send a hint: {error:#}");
            }
        }
        Ok(())
    }

    pub(crate) fn signed_hint(&self, seq: u64, head_version: String) -> Result<Hint> {
        let signed = signed_bytes(&self.inner.config.workspace_id, &self.me(), seq, &head_version)?;
        Ok(Hint {
            author: self.me(),
            seq,
            head_version,
            signature: self.inner.identity.sign(&signed).to_bytes().to_vec(),
        })
    }

    /// Takes a hint from a peer. Returns whether it names ops we lack, so a
    /// check should run now.
    pub(crate) fn receive_hint(&self, hint: &Hint) -> Result<bool> {
        ensure!(
            hint.head_version.len() <= MAX_VERSION_LENGTH,
            "hint version too long"
        );
        ensure!(
            self.current_members()?.contains(&hint.author),
            "hint from someone who isn't a member"
        );
        let signature: [u8; 64] = hint
            .signature
            .as_slice()
            .try_into()
            .context("malformed hint signature")?;
        let bytes = signed_bytes(
            &self.inner.config.workspace_id,
            &hint.author,
            hint.seq,
            &hint.head_version,
        )?;
        hint.author
            .verify(&bytes, &Signature::from_bytes(&signature))
            .context("bad hint signature")?;
        self.heard_hints()
            .insert(hint.author, (hint.seq, hint.head_version.clone()));
        Ok(hint.seq > self.state().store.last_seq(&hint.author)?)
    }

    /// Lists `heads/` once and reads the logs of authors whose head moved
    /// past what we hold, and of removed members whose frozen log we haven't
    /// finished.
    /// Repairs gaps, and rewrites our own head if it is missing or behind.
    /// Returns whether it brought ops gossip had missed.
    pub(crate) async fn check_heads(&self) -> Result<bool> {
        let me = self.me();
        let everyone: HashSet<PublicKey> = self.state().store.everyone_ever()?.into_iter().collect();
        let listed: Vec<_> = self
            .bucket()
            .heads()
            .await?
            .into_iter()
            .filter(|head| everyone.contains(&head.author))
            .collect();
        self.heal_own_head(
            listed
                .iter()
                .find(|head| head.author == me)
                .map(|head| head.version.clone()),
        )
        .await?;
        let mut to_read: Vec<(PublicKey, Option<String>)> = Vec::new();
        for head in listed {
            if head.author == me || self.seen_heads().get(&head.author) == Some(&head.version) {
                continue;
            }
            let held = self.state().store.last_seq(&head.author)?;
            let is_explained = self
                .heard_hints()
                .get(&head.author)
                .is_some_and(|(seq, version)| *version == head.version && *seq <= held);
            if is_explained {
                self.note_seen_head(&head.author, &head.version);
                continue;
            }
            match self.bucket().head(&head.author).await? {
                Some(head_now) if head_now.last_seq > held => {
                    to_read.push((head.author, Some(head.version)));
                }
                _ => self.note_seen_head(&head.author, &head.version),
            }
        }
        for author in self.refetch_gaps()? {
            to_read.push((author, None));
        }
        for author in &everyone {
            let held = self.state().store.last_seq(author)?;
            // A removed member can't fix their own head, but the removal
            // commit says how far their log goes.
            if self
                .state()
                .store
                .frozen_last_seq(author)?
                .is_some_and(|last| last > held)
            {
                to_read.push((*author, None));
            }
        }
        let mut has_found_new = false;
        let mut local_failure = None;
        for (author, head_version) in to_read {
            let held = self.state().store.last_seq(&author)?;
            match self.pull_author(&author).await? {
                // A head counts as seen only once its log was read, so a
                // failed read is tried again at the next check.
                None => {
                    if let Some(version) = head_version {
                        self.note_seen_head(&author, &version);
                    }
                }
                Some(error) => {
                    local_failure.get_or_insert(error);
                }
            }
            has_found_new |= self.state().store.last_seq(&author)? > held;
        }
        local_failure.map_or(Ok(has_found_new), Err)
    }

    /// Rewrites our head if it was replaced (or is missing) or we uploaded
    /// past it. Holds the upload lock, so it can't race an upload's own head.
    async fn heal_own_head(&self, listed_version: Option<String>) -> Result<()> {
        let me = self.me();
        let is_replaced = listed_version.as_ref() != self.seen_heads().get(&me);
        let is_pushed_past_head = self.inner.confirmed_head_seq.load(Ordering::SeqCst)
            != self.state().store.last_pushed_seq(&me)?;
        if !is_replaced && !is_pushed_past_head {
            return Ok(());
        }
        if let Some(version) = &listed_version {
            self.note_seen_head(&me, version);
        }
        let _uploading = self.inner.push_lock.lock().await;
        self.write_head().await
    }

    fn note_seen_head(&self, author: &PublicKey, version: &str) {
        self.seen_heads().insert(*author, version.to_owned());
    }

    fn seen_heads(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<PublicKey, String>> {
        self.inner
            .seen_heads
            .lock()
            .expect("seen heads lock poisoned by an earlier panic")
    }

    fn heard_hints(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<PublicKey, (u64, String)>> {
        self.inner
            .heard_hints
            .lock()
            .expect("heard hints lock poisoned by an earlier panic")
    }
}
