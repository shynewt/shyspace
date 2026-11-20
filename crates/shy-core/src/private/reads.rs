//! What the app shows about private channels and DMs.

use anyhow::Result;

use super::ids::{PrivateKind, is_message_in, private_channel_of, private_kind};
use crate::{
    ops::{Op, is_owned_by},
    storage::Store,
    views::{Channel, ChannelKind, PrivateMembers},
    workspace::Workspace,
};

impl Workspace {
    /// Who is in a private channel you hold a key for, and who owns it.
    pub fn private_members(&self, channel: &str) -> Result<PrivateMembers> {
        let state = self.state();
        let owner = state
            .store
            .private_owners(channel)?
            .pop()
            .map(|owner| owner.member);
        Ok(PrivateMembers {
            members: state
                .store
                .current_private_members(channel)?
                .iter()
                .map(|member| hex::encode(member.as_bytes()))
                .collect(),
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
                channel.is_member = store.current_private_members(&channel.id)?.contains(&me);
                channel.can_delete = is_channel_owner(store, &channel.id, &me)?;
            }
            Some(PrivateKind::Dm) => {
                channel.kind = ChannelKind::Dm;
                channel.dm_members = store
                    .dm_members(&channel.id)?
                    .iter()
                    .filter(|member| **member != me)
                    .map(|member| hex::encode(member.as_bytes()))
                    .collect();
                channel.can_delete = false;
            }
        }
        Ok(())
    }

    /// Whether you may delete `id`: in a private channel, your own messages,
    /// or anything if you own it; elsewhere, what you own or (as admin) all.
    pub(crate) fn may_delete(&self, store: &Store, id: &str) -> Result<bool> {
        let me = self.me();
        let Some(channel) = private_channel_of(id) else {
            return Ok(self.may(&Op::Delete { id: id.to_owned() }));
        };
        let is_owner =
            private_kind(channel) == Some(PrivateKind::Channel) && is_channel_owner(store, channel, &me)?;
        Ok(if is_message_in(id, channel) {
            is_owned_by(id, &me) || is_owner
        } else {
            is_owner
        })
    }
}

fn is_channel_owner(store: &Store, channel: &str, member: &iroh::PublicKey) -> Result<bool> {
    Ok(store
        .private_owners(channel)?
        .last()
        .is_some_and(|owner| owner.member == *member))
}
