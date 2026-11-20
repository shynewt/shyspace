//! Creating private channels and DMs, changing who is in them, and sealing
//! their content.

use anyhow::{Context, Result, bail, ensure};
use iroh::PublicKey;

use super::{
    crypto::{ChannelKey, Granting, Slot, commitment, new_channel_key, seal, wrap_key},
    ids::{MAX_DM_MEMBERS, PrivateKind, dm_id, new_private_channel_id, private_kind},
    ops::{GrantBody, Invitee, MAX_WRAPS_PER_GRANT, PrivateBody, PrivateOp},
};
use crate::{
    ops::{EntityId, Op, normalize_channel_name},
    storage::{Store, StoredKey},
    views::parse_member_id,
    workspace::Workspace,
};

impl Workspace {
    /// A private channel only you and `members` (member ids) can read. You
    /// own it: you add and remove people.
    pub fn create_private_channel(&self, name: &str, members: &[String]) -> Result<EntityId> {
        let name = normalize_channel_name(name)?;
        let me = self.me();
        let others = self.members_to_seal_for(members)?;
        let channel = new_private_channel_id(&me);
        let key = new_channel_key();
        let mut recipients = others.clone();
        recipients.push(me);
        self.grant(&channel, &[(1, key)], &recipients, &[])?;
        let invitees = {
            let state = self.state();
            others
                .iter()
                .map(|member| {
                    Ok(Invitee {
                        member: *member,
                        after_seq: state.store.last_seq(member)?,
                    })
                })
                .collect::<Result<_>>()?
        };
        let first_key = StoredKey {
            channel: channel.clone(),
            generation: 1,
            key,
        };
        self.write_sealed(
            first_key,
            PrivateOp::Create {
                name,
                commitment: commitment(&key),
                members: invitees,
            },
        )?;
        Ok(channel)
    }

    /// Adds someone to a private channel you own. They can read its history.
    pub fn add_to_private_channel(&self, channel: &str, member_id: &str) -> Result<()> {
        let owner_index = self.my_owner_index(channel)?;
        let [member] = self.members_to_seal_for(&[member_id.to_owned()])?[..] else {
            bail!("pick someone other than yourself");
        };
        let has_left_since_latest_key = {
            let state = self.state();
            ensure!(
                !state.store.current_private_members(channel)?.contains(&member),
                "they are already in this channel"
            );
            state.store.last_closure_generation(channel, &member)? > state.store.latest_generation(channel)?
        };
        // Their leaving ends times that start before the next key, so they
        // come back with a new one.
        if has_left_since_latest_key {
            self.rotate(channel, None)?;
        }
        let (keys, after_seq, latest) = {
            let state = self.state();
            (
                verified_keys(&state.store, channel)?,
                state.store.last_seq(&member)?,
                state.store.latest_generation(channel)?,
            )
        };
        let history: Vec<(u64, ChannelKey)> = keys.iter().map(|key| (key.generation, key.key)).collect();
        self.grant(channel, &history, &[member], &[])?;
        self.write_private(
            channel,
            PrivateOp::Add {
                invitee: Invitee { member, after_seq },
                from_generation: latest,
                owner_index,
            },
        )
    }

    /// Removes someone from a private channel you own. They keep what they
    /// already read; a new key keeps them out of what comes next.
    pub fn remove_from_private_channel(&self, channel: &str, member_id: &str) -> Result<()> {
        let member = parse_member_id(member_id)?;
        ensure!(
            member != self.me(),
            "to leave a channel you own, hand it over first"
        );
        let last_seq = {
            let state = self.state();
            ensure!(
                state.store.current_private_members(channel)?.contains(&member),
                "they aren't in this channel"
            );
            state.store.last_seq(&member)?
        };
        self.rotate(channel, Some((member, last_seq)))
    }

    /// Starts a new key for a private channel you own, for its current members.
    pub fn rotate_private_channel_key(&self, channel: &str) -> Result<()> {
        self.rotate(channel, None)
    }

    pub(crate) fn rotate(&self, channel: &str, removing: Option<(PublicKey, u64)>) -> Result<()> {
        let owner_index = self.my_owner_index(channel)?;
        let (recipients, generation) = {
            let state = self.state();
            let members = state.store.current_private_members(channel)?;
            let recipients: Vec<PublicKey> = members
                .into_iter()
                .filter(|member| removing.is_none_or(|(removed, _)| removed != *member))
                .collect();
            (recipients, state.store.latest_generation(channel)? + 1)
        };
        let key = new_channel_key();
        self.grant(channel, &[(generation, key)], &recipients, &[])?;
        let commitment = commitment(&key);
        self.write_private(
            channel,
            match removing {
                Some((member, last_seq)) => PrivateOp::Remove {
                    member,
                    last_seq,
                    generation,
                    commitment,
                    owner_index,
                },
                None => PrivateOp::Rotate {
                    generation,
                    commitment,
                    owner_index,
                },
            },
        )
    }

    /// Leaves a private channel. Its owner hands it over first, unless
    /// they're the last one in it.
    pub fn leave_private_channel(&self, channel: &str) -> Result<()> {
        {
            let state = self.state();
            let owners = state.store.private_owners(channel)?;
            let is_owner = owners.last().is_some_and(|owner| owner.member == self.me());
            let others = state
                .store
                .current_private_members(channel)?
                .len()
                .saturating_sub(1);
            ensure!(
                !is_owner || others == 0,
                "hand the channel over before leaving it"
            );
        }
        self.write_private(channel, PrivateOp::Leave)
    }

    /// Makes another member the channel's owner.
    pub fn hand_over_private_channel(&self, channel: &str, member_id: &str) -> Result<()> {
        let owner_index = self.my_owner_index(channel)?;
        let to = parse_member_id(member_id)?;
        ensure!(to != self.me(), "you already own this channel");
        self.write_private(channel, PrivateOp::HandOver { to, owner_index })
    }

    /// On channels we own, keeps out who left or was removed from the
    /// workspace: they get no newer key.
    pub(crate) fn tend_private_channels(&self) -> Result<()> {
        let me = self.me();
        let workspace_members = self.current_members()?;
        let channel_ids = self.state().store.private_channel_ids()?;
        for channel in channel_ids {
            if private_kind(&channel) != Some(PrivateKind::Channel) || self.my_owner_index(&channel).is_err()
            {
                continue;
            }
            let (gone, have_left) = {
                let state = self.state();
                let mut gone = Vec::new();
                for member in state.store.current_private_members(&channel)? {
                    if member != me && !workspace_members.contains(&member) {
                        gone.push((member, state.store.last_seq(&member)?));
                    }
                }
                (gone, !state.store.left_since_latest_key(&channel)?.is_empty())
            };
            for removed in &gone {
                self.rotate(&channel, Some(*removed))?;
            }
            if gone.is_empty() && have_left {
                self.rotate(&channel, None)?;
            }
        }
        Ok(())
    }

    /// The DM with exactly these members (member ids) and you. Opening it
    /// again finds the same one.
    pub fn open_dm(&self, members: &[String]) -> Result<EntityId> {
        let mut everyone = self.members_to_seal_for(members)?;
        everyone.push(self.me());
        everyone.sort_unstable_by_key(|member| *member.as_bytes());
        ensure!(
            (2..=MAX_DM_MEMBERS).contains(&everyone.len()),
            "a DM is you and 1 to {} others",
            MAX_DM_MEMBERS - 1
        );
        let channel = dm_id(&self.inner.config.workspace_id, &everyone);
        if !self.state().store.channel_keys(Some(&channel))?.is_empty() {
            return Ok(channel);
        }
        self.grant(&channel, &[(0, new_channel_key())], &everyone, &everyone)?;
        Ok(channel)
    }

    /// Seals `op` into `channel` with the newest key we may use there.
    pub(crate) fn write_private(&self, channel: &str, op: PrivateOp) -> Result<()> {
        let key = sending_key(&self.state().store, channel)?;
        self.write_sealed(key, op)
    }

    fn write_sealed(&self, key: StoredKey, op: PrivateOp) -> Result<()> {
        let workspace_id = self.inner.config.workspace_id.clone();
        let me = self.me();
        self.write_with(|_, seq| {
            let body = PrivateBody {
                channel: key.channel.clone(),
                generation: key.generation,
                op,
            };
            let slot = Slot {
                workspace_id: &workspace_id,
                author: &me,
                seq,
            };
            let payload = seal(&key.key, slot, &postcard::to_stdvec(&body)?)?;
            Ok(Op::Sealed { payload })
        })
    }

    /// Wraps each key for each recipient, in as many grants as it takes.
    fn grant(
        &self,
        channel: &str,
        keys: &[(u64, ChannelKey)],
        recipients: &[PublicKey],
        dm_members: &[PublicKey],
    ) -> Result<()> {
        let me = self.me();
        let granting = Granting {
            workspace_id: &self.inner.config.workspace_id,
            granter: &me,
        };
        let mut wraps = Vec::new();
        for recipient in recipients {
            let public = self
                .state()
                .store
                .sealing_key_of(recipient)?
                .context("someone hasn't updated shyspace yet, so they can't join private channels")?;
            for (generation, key) in keys {
                let grant = GrantBody {
                    channel: channel.to_owned(),
                    generation: *generation,
                    key: *key,
                    dm_members: dm_members.to_vec(),
                };
                wraps.push(wrap_key(granting, &public, &postcard::to_stdvec(&grant)?)?);
            }
        }
        for chunk in wraps.chunks(MAX_WRAPS_PER_GRANT) {
            self.write(Op::KeyGrant {
                wraps: chunk.to_vec(),
            })?;
        }
        Ok(())
    }

    /// Current workspace members other than you, each with a sealing key.
    fn members_to_seal_for(&self, member_ids: &[String]) -> Result<Vec<PublicKey>> {
        let me = self.me();
        let current = self.current_members()?;
        let mut members = Vec::new();
        for id in member_ids {
            let member = parse_member_id(id)?;
            ensure!(current.contains(&member), "{id} isn't a member of this workspace");
            if member != me && !members.contains(&member) {
                members.push(member);
            }
        }
        Ok(members)
    }

    fn my_owner_index(&self, channel: &str) -> Result<u32> {
        ensure!(
            private_kind(channel) == Some(PrivateKind::Channel),
            "only private channels have owners"
        );
        let owners = self.state().store.private_owners(channel)?;
        match owners.last() {
            Some(owner) if owner.member == self.me() => Ok(u32::try_from(owners.len() - 1)?),
            _ => bail!("only the channel's owner can do that"),
        }
    }
}

/// Keys of `channel` its owners vouched for, oldest first. For a DM, every
/// key we hold (they were checked when granted).
fn verified_keys(store: &Store, channel: &str) -> Result<Vec<StoredKey>> {
    let mut verified = Vec::new();
    for key in store.channel_keys(Some(channel))? {
        let is_vouched = match private_kind(channel) {
            Some(PrivateKind::Dm) => true,
            _ => store.commitment_of(channel, key.generation)? == Some(commitment(&key.key)),
        };
        if is_vouched {
            verified.push(key);
        }
    }
    Ok(verified)
}

fn sending_key(store: &Store, channel: &str) -> Result<StoredKey> {
    verified_keys(store, channel)?
        .pop()
        .context("you aren't a member of this channel")
}
