//! Local changes: each one becomes a signed, encrypted op.

use anyhow::{Context, Result, bail};
use tracing::warn;

use super::{Profile, Workspace};
use crate::{
    crypto,
    ops::{EntityId, Envelope, Op, OpBody, VerifiedOp, new_owned_id, now_ms},
    pages::{self, Edit},
};

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
        let name = name
            .trim()
            .trim_start_matches('#')
            .to_lowercase()
            .replace(' ', "-");
        if name.is_empty() {
            bail!("channel name is empty");
        }
        let id = new_owned_id(&self.me());
        self.write(Op::ChannelCreate { id: id.clone(), name })?;
        Ok(id)
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
        Ok(id)
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
