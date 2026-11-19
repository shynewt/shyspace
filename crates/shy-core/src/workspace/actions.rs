//! Local changes: each one becomes a signed, encrypted op.

use anyhow::{Context, Result, bail};
use tracing::warn;

use super::{Profile, Workspace};
use crate::{
    crypto,
    ops::{EntityId, Envelope, Op, OpBody, VerifiedOp, new_owned_id, normalize_channel_name, now_ms},
    pages::{self, Edit},
    storage::ItemKind,
    views::{Change, Message, NotifyLevel},
};

const MAX_MENTION_SCAN: u32 = 500;

impl Workspace {
    /// Seals, stores and applies a local change, then sends it live. The
    /// bucket upload happens on the next sync pass.
    pub(crate) fn write(&self, op: Op) -> Result<()> {
        let (envelope, change) = {
            let state = self.state();
            if !state.mls.is_active() {
                bail!("you are no longer a member of this workspace");
            }
            let epoch = state.mls.epoch()?;
            let key = state
                .store
                .epoch_key(epoch)?
                .context("missing key for the current epoch")?;
            let author = self.me();
            if !self.may(&op) {
                bail!("only its owner or the admin can do that");
            }
            state.store.atomically(|store| {
                let body = OpBody {
                    lamport: store.next_lamport()?,
                    timestamp_ms: now_ms()?,
                    op,
                };
                let seq = store.next_seq(&author)?;
                let envelope = crypto::seal(&self.inner.identity, &key, epoch, seq, &body)?;
                let change = store.apply(&VerifiedOp { author, seq, body })?;
                store.insert_applied(&envelope, false)?;
                Ok((envelope, change))
            })?
        };
        if let Some(change) = change {
            self.emit(change);
        }
        self.send_live(envelope);
        self.inner.wake.notify_one();
        Ok(())
    }

    fn send_live(&self, envelope: Envelope) {
        if self.inner.net.is_none() {
            return;
        }
        let workspace = self.clone();
        tokio::spawn(async move {
            if let Some(net) = &workspace.inner.net
                && let Err(error) = net.broadcast(envelope).await
            {
                warn!("live send failed, the bucket will deliver it: {error:#}");
            }
        });
    }

    pub fn create_channel(&self, name: &str) -> Result<EntityId> {
        let id = new_owned_id(&self.me());
        self.write(Op::ChannelCreate {
            id: id.clone(),
            name: normalize_channel_name(name)?,
        })?;
        Ok(id)
    }

    pub fn rename_channel(&self, channel: &str, name: &str) -> Result<()> {
        self.require_item(channel, ItemKind::Channel)?;
        self.write(Op::ChannelRename {
            id: channel.to_owned(),
            name: normalize_channel_name(name)?,
        })
    }

    /// Deletes the channel and every message in it. Owner or admin only.
    pub fn delete_channel(&self, channel: &str) -> Result<()> {
        self.require_item(channel, ItemKind::Channel)?;
        self.write(Op::Delete {
            id: channel.to_owned(),
        })
    }

    pub fn send_message(&self, channel: &str, text: &str, reply_to: Option<EntityId>) -> Result<EntityId> {
        let text = text.trim();
        if text.is_empty() {
            bail!("message is empty");
        }
        let id = new_owned_id(&self.me());
        self.write(Op::Message {
            id: id.clone(),
            channel: channel.to_owned(),
            text: text.to_owned(),
            reply_to,
        })?;
        // Replying means you've read what came before.
        self.state().store.mark_read(channel, &id)?;
        Ok(id)
    }

    /// Your unread messages that mention you, in every channel, and marks
    /// them read: each is returned once. Looks at the latest
    /// `MAX_MENTION_SCAN` messages of each channel.
    pub fn take_unread_mentions(&self) -> Result<Vec<Message>> {
        let mut found = Vec::new();
        for channel in self.channels()? {
            if channel.unread_mentions == 0 {
                continue;
            }
            let mentioning: Vec<Message> = self
                .messages(&channel.id, None, MAX_MENTION_SCAN)?
                .into_iter()
                .filter(|message| {
                    message.mentions_me
                        && channel
                            .last_read
                            .as_ref()
                            .is_none_or(|read| read.precedes(message))
                })
                .collect();
            if let Some(last) = mentioning.last() {
                self.state().store.mark_read(&channel.id, &last.id)?;
            }
            found.extend(mentioning);
        }
        if !found.is_empty() {
            self.emit(Change::Channels);
        }
        Ok(found)
    }

    /// Which new messages in `channel` alert you. Personal, like read positions.
    pub fn set_notify_level(&self, channel: &str, level: NotifyLevel) -> Result<()> {
        self.require_item(channel, ItemKind::Channel)?;
        self.state().store.set_notify_level(channel, level)?;
        self.emit(Change::Channels);
        Ok(())
    }

    /// Moves your read position in `channel` up to `message`. Personal: kept
    /// on this device, never sent to others.
    pub fn mark_read(&self, channel: &str, message: &str) -> Result<()> {
        self.state().store.mark_read(channel, message)?;
        self.emit(Change::Channels);
        Ok(())
    }

    /// Replaces a message's text. Its author only.
    pub fn edit_message(&self, message: &str, text: &str) -> Result<()> {
        let text = text.trim();
        if text.is_empty() {
            bail!("message is empty");
        }
        self.require_item(message, ItemKind::Message)?;
        self.write(Op::MessageEdit {
            id: message.to_owned(),
            text: text.to_owned(),
        })
    }

    /// Author or admin only.
    pub fn delete_message(&self, message: &str) -> Result<()> {
        self.require_item(message, ItemKind::Message)?;
        self.write(Op::Delete {
            id: message.to_owned(),
        })
    }

    pub fn react(&self, message: &str, emoji: &str, active: bool) -> Result<()> {
        self.write(Op::Reaction {
            message: message.to_owned(),
            emoji: emoji.to_owned(),
            active,
        })
    }

    pub fn create_page(&self, title: &str, parent: Option<EntityId>) -> Result<EntityId> {
        let id = new_owned_id(&self.me());
        self.write(Op::PageCreate {
            id: id.clone(),
            parent,
            title: title.trim().to_owned(),
        })?;
        Ok(id)
    }

    pub fn rename_page(&self, page: &str, title: &str) -> Result<()> {
        self.write(Op::PageRename {
            id: page.to_owned(),
            title: title.trim().to_owned(),
        })
    }

    /// Its sub-pages stay, moving to the top level. Owner or admin only.
    pub fn delete_page(&self, page: &str) -> Result<()> {
        self.require_item(page, ItemKind::Page)?;
        self.write(Op::Delete { id: page.to_owned() })
    }

    /// Edits and deletes name an existing item of the expected kind, so a
    /// mistaken id fails here instead of silently doing nothing.
    fn require_item(&self, id: &str, kind: ItemKind) -> Result<()> {
        let found = self.state().store.item_kind(id)?;
        if found != Some(kind) {
            bail!("no such {} here", kind.noun());
        }
        Ok(())
    }

    /// A Yjs update produced by an editor.
    pub fn apply_doc_update(&self, page: &str, update: Vec<u8>) -> Result<()> {
        self.write(Op::DocUpdate {
            page: page.to_owned(),
            update,
        })
    }

    pub fn edit_page(&self, page: &str, edit: Edit) -> Result<()> {
        let state = self.state().store.page_state(page)?;
        let update = pages::edit(state.as_deref(), edit)?;
        self.apply_doc_update(page, update)
    }

    pub fn set_profile(&self, profile: Profile<'_>) -> Result<()> {
        self.write(Op::Profile {
            name: profile.name.trim().to_owned(),
            is_bot: profile.is_bot,
        })
    }
}
