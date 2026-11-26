//! What the app shows about private channels and DMs.

use anyhow::Result;

use super::ids::{PrivateKind, is_message_in, private_channel_of, private_kind};
use crate::{
    ops::{Op, is_owned_by},
    storage::{Ending, Store},
    views::{Channel, ChannelKind, PrivateMembers, member_id},
    workspace::Workspace,
};

impl Workspace {
    /// Who is in a private channel you hold a key for, and who owns it.
    pub fn private_members(&self, channel: &str) -> Result<PrivateMembers> {
        let state = self.state();
        let owner = state.store.current_owner(channel)?;
        let members = state.store.private_members(channel, Ending::Left)?;
        let mut invited = Vec::new();
        for member in &members {
            if !state.store.has_joined(channel, member)? {
                invited.push(member_id(member));
            }
        }
        Ok(PrivateMembers {
            members: members.iter().map(member_id).collect(),
            invited,
            owner: owner.map(|owner| hex::encode(owner.as_bytes())),
            is_owner: owner == Some(self.me()),
        })
    }

    /// Fills in what's particular to a private channel or DM.
    pub(crate) fn describe_private(&self, store: &Store, channel: &mut Channel) -> Result<()> {
        let me = self.me();
        match private_kind(&channel.id) {
            None => {}
            Some(PrivateKind::Channel) => {
                channel.kind = ChannelKind::Private;
                channel.is_member = store.private_members(&channel.id, Ending::Left)?.contains(&me);
                channel.is_missing_key = channel.is_member && !store.holds_newest_key(&channel.id)?;
                channel.can_delete = store.current_owner(&channel.id)? == Some(me);
            }
            Some(PrivateKind::Dm) => {
                channel.dm_members = store
                    .dm_members(&channel.id)?
                    .iter()
                    .filter(|member| **member != me)
                    .map(|member| hex::encode(member.as_bytes()))
                    .collect();
                channel.kind = match channel.dm_members.is_empty() {
                    true => ChannelKind::Notes,
                    false => ChannelKind::Dm,
                };
                channel.can_delete = false;
            }
        }
        Ok(())
    }

    /// Whether you may delete `id`: in a private channel, your own messages,
    /// or anything if you own it; elsewhere, what you own or (as admin) all.
    pub(crate) fn may_delete(&self, store: &Store, epoch: u64, id: &str) -> Result<bool> {
        let me = self.me();
        let Some(channel) = private_channel_of(id) else {
            return self.may(store, epoch, &Op::Delete { id: id.to_owned() });
        };
        let is_owner =
            private_kind(channel) == Some(PrivateKind::Channel) && store.current_owner(channel)? == Some(me);
        Ok(if is_message_in(id, channel) {
            is_owned_by(id, &me) || is_owner
        } else {
            is_owner
        })
    }
}
