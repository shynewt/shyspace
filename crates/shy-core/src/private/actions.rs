//! Creating private channels and DMs, changing who is in them, and sealing
//! their content.

use anyhow::{Context, Result, bail, ensure};
use iroh::PublicKey;

use super::{
    crypto::{ChannelKey, Granting, Slot, commitment, new_channel_key, seal, wrap_key},
    ids::{MAX_DM_MEMBERS, PrivateKind, dm_id, private_channel_id, private_kind},
    ops::{GrantBody, Invitee, MAX_WRAPS_PER_GRANT, PrivateBody, PrivateOp},
    rules::Verdict,
};
use crate::{
    ops::{EntityId, Op, is_owned_by, normalize_channel_name},
    storage::{Ending, Store, StoredKey},
    views::parse_member_id,
    workspace::{OpMaker, Workspace},
};

impl Workspace {
    /// Private channels and DMs are each person's first device's until
    /// they go by person.
    fn ensure_may_use_private(&self) -> Result<()> {
        ensure!(
            self.my_person() == self.me(),
            "private channels and DMs aren't available on a linked device yet"
        );
        Ok(())
    }

    /// A private channel only you and `members` (member ids) can read. You
    /// own it: you add and remove people.
    pub fn create_private_channel(&self, name: &str, members: &[String]) -> Result<EntityId> {
        self.ensure_may_use_private()?;
        let name = normalize_channel_name(name)?;
        let me = self.me();
        let others = self.members_to_seal_for(members)?;
        let invitees: Vec<Invitee> = {
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
        let key = new_channel_key();
        let announced = commitment(&key);
        let channel = private_channel_id(&me, &announced, &name, &invitees)?;
        let mut recipients = others;
        recipients.push(me);
        self.grant(&channel, &[(1, key)], &recipients, &[])?;
        let first_key = StoredKey {
            channel: channel.clone(),
            generation: 1,
            key,
        };
        self.write_sealed(
            first_key,
            PrivateOp::Create {
                name,
                commitment: announced,
                members: invitees,
            },
        )?;
        Ok(channel)
    }

    /// Adds someone to a private channel you own. They can read its history.
    pub fn add_to_private_channel(&self, channel: &str, member_id: &str) -> Result<()> {
        self.ensure_may_use_private()?;
        self.ensure_owner(channel)?;
        let [member] = self.members_to_seal_for(&[member_id.to_owned()])?[..] else {
            bail!("pick someone other than yourself");
        };
        let (is_listed, has_left) = {
            let store = &self.state().store;
            (
                store.owner_membership(channel, &member)?.is_some(),
                !store.private_members(channel, Ending::Left)?.contains(&member),
            )
        };
        ensure!(!is_listed || has_left, "they are already in this channel");
        // Someone who left still counts until the owner removes them, which
        // also makes a key they don't have.
        if is_listed {
            self.rotate(channel, Some(member))?;
        }
        let (keys, after_seq, latest, position) = {
            let store = &self.state().store;
            (
                vouched_keys(store, channel)?,
                store.last_seq(&member)?,
                store.latest_generation(channel)?,
                store.next_position(channel)?,
            )
        };
        let writing_key = self.writing_key(channel)?;
        let history: Vec<(u64, ChannelKey)> = keys.iter().map(|key| (key.generation, key.key)).collect();
        // Written together: a refused add takes its grant back with it.
        let mut makers = self.grant_makers(channel, &history, &[member], &[])?;
        makers.push(self.sealed_maker(
            writing_key,
            PrivateOp::Add {
                invitee: Invitee { member, after_seq },
                from_generation: latest,
                position,
            },
        ));
        self.write_together(makers)
    }

    /// Removes someone from a private channel you own. They keep what they
    /// already read; a new key keeps them out of what comes next.
    pub fn remove_from_private_channel(&self, channel: &str, member_id: &str) -> Result<()> {
        self.ensure_may_use_private()?;
        let member = parse_member_id(member_id)?;
        ensure!(
            member != self.me(),
            "to leave a channel you own, hand it over first"
        );
        ensure!(
            self.state().store.owner_membership(channel, &member)?.is_some(),
            "they aren't in this channel"
        );
        self.rotate(channel, Some(member))
    }

    /// Starts a new key for a private channel you own, for its current members.
    pub fn rotate_private_channel_key(&self, channel: &str) -> Result<()> {
        self.ensure_may_use_private()?;
        self.rotate(channel, None)
    }

    fn rotate(&self, channel: &str, removing: Option<PublicKey>) -> Result<()> {
        self.ensure_owner(channel)?;
        let (recipients, generation, last_seq) = {
            let store = &self.state().store;
            let recipients: Vec<PublicKey> = store
                .private_members(channel, Ending::Left)?
                .into_iter()
                .filter(|member| Some(*member) != removing)
                .collect();
            let last_seq = removing.map(|member| store.last_seq(&member)).transpose()?;
            (recipients, store.latest_generation(channel)? + 1, last_seq)
        };
        let key = new_channel_key();
        self.grant(channel, &[(generation, key)], &recipients, &[])?;
        let commitment = commitment(&key);
        self.write_owner_op(channel, |position| match (removing, last_seq) {
            (Some(member), Some(last_seq)) => PrivateOp::Remove {
                member,
                last_seq,
                generation,
                commitment,
                position,
            },
            _ => PrivateOp::Rotate {
                generation,
                commitment,
                position,
            },
        })
    }

    /// Leaves a private channel. Its owner hands it over first, unless
    /// they're the last one in it.
    pub fn leave_private_channel(&self, channel: &str) -> Result<()> {
        self.ensure_may_use_private()?;
        {
            let store = &self.state().store;
            let is_owner = store.current_owner(channel)? == Some(self.me());
            let others = store
                .private_members(channel, Ending::Left)?
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
        self.ensure_may_use_private()?;
        self.ensure_owner(channel)?;
        let to = parse_member_id(member_id)?;
        ensure!(to != self.me(), "you already own this channel");
        self.write_owner_op(channel, |position| PrivateOp::HandOver { to, position })
    }

    /// The DM with exactly these members (member ids) and you. Opening it
    /// again finds the same one.
    pub fn open_dm(&self, members: &[String]) -> Result<EntityId> {
        self.ensure_may_use_private()?;
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

    /// Your notes to yourself: a DM whose only member is you, made the first
    /// time. Its key is granted to you alone, so only you can read or write it.
    pub fn open_notes(&self) -> Result<EntityId> {
        self.ensure_may_use_private()?;
        let me = self.me();
        let channel = dm_id(&self.inner.config.workspace_id, &[me]);
        if !self.state().store.channel_keys(Some(&channel))?.is_empty() {
            return Ok(channel);
        }
        self.grant(&channel, &[(0, new_channel_key())], &[me], &[me])?;
        Ok(channel)
    }

    /// Takes over channels whose removed owner we succeed, passes on those
    /// we own but left, and on channels we own keeps out who left or was
    /// removed from the workspace: the owner removes them, which makes a key
    /// they don't get.
    pub(crate) fn tend_private_channels(&self) -> Result<()> {
        let me = self.me();
        let workspace_members = self.current_members()?;
        let channel_ids = self.state().store.private_channel_ids()?;
        for channel in channel_ids {
            if private_kind(&channel) != Some(PrivateKind::Channel) {
                continue;
            }
            self.confirm_joined(&channel)?;
            self.claim_if_successor(&channel)?;
            if self.ensure_owner(&channel).is_err() || self.pass_on_if_left(&channel, &workspace_members)? {
                continue;
            }
            let to_remove: Vec<PublicKey> = {
                let store = &self.state().store;
                let staying = store.private_members(&channel, Ending::Left)?;
                store
                    .private_members(&channel, Ending::RemovedByOwner)?
                    .into_iter()
                    .filter(|member| {
                        *member != me && (!staying.contains(member) || !workspace_members.contains(member))
                    })
                    .collect()
            };
            for member in to_remove {
                self.rotate(&channel, Some(member))?;
            }
        }
        Ok(())
    }

    /// Writes that we joined `channel` once we hold the key for our time in
    /// it and have read a message there since we were added, unless an op of
    /// ours there already shows it. Only our own reading triggers it: an add,
    /// or a re-add after a removal, alone never does.
    fn confirm_joined(&self, channel: &str) -> Result<()> {
        let me = self.me();
        let should_write = {
            let store = &self.state().store;
            match (
                store.owner_membership(channel, &me)?,
                sending_key(store, channel).ok(),
            ) {
                (Some(membership), Some(key)) => {
                    key.generation >= membership.from_generation
                        && store.private_members(channel, Ending::Left)?.contains(&me)
                        && !store.has_joined(channel, &me)?
                        && store.is_covered(channel, &me, store.next_seq(&me)?, key.generation)?
                        && store.has_read_since_added(channel, &me)?
                }
                _ => false,
            }
        };
        if should_write {
            self.write_private(channel, PrivateOp::Joined)?;
        }
        Ok(())
    }

    /// Writes our claim on `channel` if its owner was removed and we are
    /// next in line, judged exactly as every other device will judge it.
    fn claim_if_successor(&self, channel: &str) -> Result<()> {
        let verdict = {
            let state = self.state();
            let position = state.store.next_position(channel)?;
            let epoch = state.mls.epoch()?;
            state
                .store
                .judge_succession(channel, position, &self.me(), epoch, self.apply_context())?
        };
        if matches!(verdict, Verdict::Apply) {
            self.write_owner_op(channel, |position| PrivateOp::Succeed { position })?;
        }
        Ok(())
    }

    /// An owner who left (only a successor can be one) hands the channel to
    /// the earliest member still in it. Returns whether it did.
    fn pass_on_if_left(&self, channel: &str, workspace_members: &[PublicKey]) -> Result<bool> {
        let me = self.me();
        let next_owner = {
            let store = &self.state().store;
            let staying = store.private_members(channel, Ending::Left)?;
            if staying.contains(&me) {
                return Ok(false);
            }
            store.members_in_owner_order(channel)?.into_iter().find(|member| {
                *member != me && staying.contains(member) && workspace_members.contains(member)
            })
        };
        let Some(to) = next_owner else {
            return Ok(false);
        };
        self.write_owner_op(channel, |position| PrivateOp::HandOver { to, position })?;
        Ok(true)
    }

    /// Seals `op` into `channel`. A delete of someone else's message (or of
    /// the channel) is the owner's to make.
    pub(crate) fn write_private_op(&self, channel: &str, op: Op) -> Result<()> {
        match op {
            Op::Delete { id } if !is_owned_by(&id, &self.me()) => {
                self.ensure_owner(channel)?;
                self.write_owner_op(channel, |position| PrivateOp::OwnerDelete { id, position })
            }
            other => self.write_private(channel, PrivateOp::Content(other)),
        }
    }

    /// Writes the channel's next owner op.
    fn write_owner_op(&self, channel: &str, make_op: impl FnOnce(u64) -> PrivateOp) -> Result<()> {
        let position = self.state().store.next_position(channel)?;
        self.write_private(channel, make_op(position))
    }

    /// Seals `op` into `channel` with the key we write there with.
    fn write_private(&self, channel: &str, op: PrivateOp) -> Result<()> {
        let key = self.writing_key(channel)?;
        self.write_sealed(key, op)
    }

    /// Makes `op`, content for `channel`, sealed with the key we write there
    /// with once its sequence number is known.
    pub(crate) fn private_content_maker(&self, channel: &str, op: Op) -> Result<OpMaker<'static>> {
        let key = self.writing_key(channel)?;
        Ok(self.sealed_maker(key, PrivateOp::Content(op)))
    }

    fn writing_key(&self, channel: &str) -> Result<StoredKey> {
        match private_kind(channel) {
            Some(PrivateKind::Dm) => self.own_dm_key(channel),
            _ => sending_key(&self.state().store, channel),
        }
    }

    /// The key we write a DM with: one we made ourselves, made the first
    /// time. A key someone else made may have reached only some members.
    /// Safe only while every DM grant this device writes goes to all of the
    /// DM's current members at once, as here and in `open_dm`.
    fn own_dm_key(&self, channel: &str) -> Result<StoredKey> {
        let me = self.me();
        let members = {
            let store = &self.state().store;
            if let Some(key) = store.dm_key_granted_by(channel, &me)? {
                return Ok(key);
            }
            store.dm_members(channel)?
        };
        ensure!(members.contains(&me), "you aren't a member of this channel");
        let workspace_members = self.current_members()?;
        let recipients: Vec<PublicKey> = members
            .iter()
            .copied()
            .filter(|member| workspace_members.contains(member))
            .collect();
        self.grant(channel, &[(0, new_channel_key())], &recipients, &members)?;
        self.state()
            .store
            .dm_key_granted_by(channel, &me)?
            .context("a DM key we just granted ourselves is missing")
    }

    fn write_sealed(&self, key: StoredKey, op: PrivateOp) -> Result<()> {
        self.write_together(vec![self.sealed_maker(key, op)])
    }

    fn sealed_maker(&self, key: StoredKey, op: PrivateOp) -> OpMaker<'static> {
        let workspace_id = self.inner.config.workspace_id.clone();
        let me = self.me();
        Box::new(move |_, seq| {
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
            let payload = seal(key.labeled(), slot, &postcard::to_stdvec(&body)?)?;
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
        self.write_together(self.grant_makers(channel, keys, recipients, dm_members)?)
    }

    /// The grants that wrap each key for each recipient, in as many ops as
    /// it takes, to be written together with what they belong to.
    fn grant_makers(
        &self,
        channel: &str,
        keys: &[(u64, ChannelKey)],
        recipients: &[PublicKey],
        dm_members: &[PublicKey],
    ) -> Result<Vec<OpMaker<'static>>> {
        let me = self.me();
        let granting = Granting {
            workspace_id: &self.inner.config.workspace_id,
            granter: &me,
        };
        let mut wraps = Vec::new();
        for recipient in recipients {
            // A linked device can't hold a sealing key until private channels
            // go by person; one an add named before its link stays listed,
            // and gets no key, rather than stopping every rotation.
            if self.state().store.person_of(recipient)? != *recipient {
                continue;
            }
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
        Ok(wraps
            .chunks(MAX_WRAPS_PER_GRANT)
            .map(|chunk| {
                let wraps = chunk.to_vec();
                Box::new(move |_: &Store, _: u64| Ok(Op::KeyGrant { wraps })) as OpMaker<'static>
            })
            .collect())
    }

    /// Current workspace members other than you, none of them a linked
    /// device.
    fn members_to_seal_for(&self, member_ids: &[String]) -> Result<Vec<PublicKey>> {
        let me = self.me();
        let current = self.current_members()?;
        let mut members = Vec::new();
        for id in member_ids {
            let member = parse_member_id(id)?;
            ensure!(current.contains(&member), "{id} isn't a member of this workspace");
            ensure!(
                self.state().store.person_of(&member)? == member,
                "that's a linked device: private channels and DMs aren't available on linked devices yet"
            );
            if member != me && !members.contains(&member) {
                members.push(member);
            }
        }
        Ok(members)
    }

    fn ensure_owner(&self, channel: &str) -> Result<()> {
        ensure!(
            private_kind(channel) == Some(PrivateKind::Channel),
            "only private channels have owners"
        );
        ensure!(
            self.state().store.current_owner(channel)? == Some(self.me()),
            "only the channel's owner can do that"
        );
        Ok(())
    }
}

/// Keys of private channel `channel` its owners vouched for, oldest first.
fn vouched_keys(store: &Store, channel: &str) -> Result<Vec<StoredKey>> {
    let mut vouched = Vec::new();
    for key in store.channel_keys(Some(channel))? {
        if store.commitment_of(channel, key.generation)? == Some(commitment(&key.key)) {
            vouched.push(key);
        }
    }
    Ok(vouched)
}

fn sending_key(store: &Store, channel: &str) -> Result<StoredKey> {
    vouched_keys(store, channel)?
        .pop()
        .context("you aren't a member of this channel")
}
