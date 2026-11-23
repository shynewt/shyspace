//! Local changes: each one becomes a signed, encrypted op.

use anyhow::{Context, Result, bail, ensure};
use tracing::warn;

use super::{Profile, Workspace};
use crate::{
    crypto,
    ops::{
        EntityId, Envelope, MAX_EMOJI_CHARS, MAX_MESSAGE_CHARS, MAX_OP_BYTES, MAX_PAGE_TITLE_CHARS,
        MAX_PAGE_UPDATE_BYTES, MAX_PROFILE_NAME_CHARS, Op, OpBody, VerifiedOp, clean_display_name,
        clean_emoji, clean_page_title, new_owned_id, normalize_channel_name, now_ms,
    },
    pages::{self, Edit},
    private::ids::{new_private_message_id, private_channel_of, private_kind},
    profile::ProfileDetails,
    storage::{ItemKind, Store},
    views::{Change, Draft, Message, NotifyLevel},
};

const MAX_MENTION_SCAN: u32 = 500;

/// Makes an op once its sequence number is known (sealed content is bound
/// to it), inside the transaction that stores it.
pub(crate) type OpMaker<'a> = Box<dyn FnOnce(&Store, u64) -> Result<Op> + 'a>;

impl Workspace {
    /// Seals, stores and applies a local change, then sends it live. The
    /// bucket upload happens on the next sync pass.
    /// Content of a private channel or DM is sealed for its members.
    pub(crate) fn write(&self, op: Op) -> Result<()> {
        let private_channel = op
            .named_ids()
            .into_iter()
            .find_map(private_channel_of)
            .map(str::to_owned);
        match private_channel {
            Some(channel) => self.write_private_op(&channel, op),
            None => self.write_with(|_, _| Ok(op)),
        }
    }

    /// The op maker for a change to content (a message, a file): sealed for
    /// its members in a private channel or DM, where making the key for a
    /// DM may write a grant first. Owner deletes go through
    /// [`Workspace::write`].
    pub(crate) fn content_maker(&self, op: Op) -> Result<OpMaker<'static>> {
        let private_channel = op
            .named_ids()
            .into_iter()
            .find_map(private_channel_of)
            .map(str::to_owned);
        match private_channel {
            Some(channel) => self.private_content_maker(&channel, op),
            None => Ok(Box::new(move |_, _| Ok(op))),
        }
    }

    /// Like [`Workspace::write`], for an op that needs its own sequence number
    /// (sealed content is bound to it).
    pub(crate) fn write_with(&self, make_op: impl FnOnce(&Store, u64) -> Result<Op>) -> Result<()> {
        self.write_together(vec![Box::new(make_op)])
    }

    /// Seals, stores and applies changes that belong together, each with the
    /// next sequence number: all of them or none. They go out live only once
    /// they are all stored.
    pub(crate) fn write_together(&self, makers: Vec<OpMaker<'_>>) -> Result<()> {
        let (envelopes, changes) = {
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
            let context = self.apply_context();
            state.store.atomically(|store| {
                let mut envelopes = Vec::new();
                let mut changes = Vec::new();
                for make_op in makers {
                    let seq = store.next_seq(&author)?;
                    let op = make_op(store, seq)?;
                    if !self.may(&op) {
                        bail!("only its owner or the admin can do that");
                    }
                    let is_sealed = matches!(op, Op::Sealed { .. });
                    let body = OpBody {
                        lamport: store.next_lamport()?,
                        timestamp_ms: now_ms()?,
                        op,
                    };
                    let envelope = crypto::seal(&self.inner.identity, &key, epoch, seq, &body)?;
                    ensure!(envelope.fits_size_limit(&body.op), too_large(&body.op));
                    let applied = store.apply_any(
                        &VerifiedOp {
                            author,
                            seq,
                            epoch,
                            body,
                        },
                        context,
                    )?;
                    // Our own private op must count, or nobody else would see it.
                    ensure!(
                        !is_sealed || store.is_private_applied(&author, seq)?,
                        "that isn't allowed in this channel"
                    );
                    store.insert_applied(&envelope, false)?;
                    changes.extend(applied.changes);
                    if applied.needs_rebuild {
                        store.rebuild_views(context)?;
                        changes.extend(store.all_view_changes()?);
                    }
                    envelopes.push(envelope);
                }
                Ok((envelopes, changes))
            })?
        };
        for change in changes {
            self.emit(change);
        }
        for envelope in envelopes {
            self.send_live(envelope);
        }
        self.inner.push_now.notify_one();
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
        let text = checked_message_text(text)?;
        if let Some(target) = &reply_to {
            self.ensure_can_reply_to(channel, target)?;
        }
        let id = self.new_item_id(channel);
        self.post_message(channel, &id, text, reply_to)?;
        Ok(id)
    }

    /// A reply belongs in a thread of the same channel: one to a message in
    /// no thread (its replies loop, or run too deep) would show nowhere. One
    /// to a message we don't have yet shows in the channel until it arrives,
    /// as before.
    pub(super) fn ensure_can_reply_to(&self, channel: &str, target: &str) -> Result<()> {
        let store = &self.state().store;
        if store.item_kind(target)? != Some(ItemKind::Message) {
            return Ok(());
        }
        ensure!(
            store.message_channel(target)?.as_deref() == Some(channel),
            "replies stay in the channel of the message they answer"
        );
        ensure!(
            store.thread_root(target)?.is_some(),
            "that message isn't in a thread you can reply in"
        );
        Ok(())
    }

    /// An id for something new in `channel`, owned by this member. Private
    /// channels' ids name the channel, so their content can't leave it.
    pub(crate) fn new_item_id(&self, channel: &str) -> EntityId {
        match private_kind(channel) {
            Some(_) => new_private_message_id(channel, &self.me()),
            None => new_owned_id(&self.me()),
        }
    }

    /// Writes message `id`, already checked, and clears the draft it came from.
    pub(crate) fn post_message(
        &self,
        channel: &str,
        id: &str,
        text: &str,
        reply_to: Option<EntityId>,
    ) -> Result<()> {
        self.write(Op::Message {
            id: id.to_owned(),
            channel: channel.to_owned(),
            text: text.to_owned(),
            reply_to,
        })?;
        self.after_posting(channel, id)
    }

    /// What posting message `id` means for you: you've read what came
    /// before, and the draft it came from is done.
    pub(crate) fn after_posting(&self, channel: &str, id: &str) -> Result<()> {
        {
            let state = self.state();
            state.store.mark_read(channel, id)?;
            let draft_target = match state.store.thread_root(id)? {
                Some(root) if root != id => {
                    state.store.mark_thread_read(&root, id)?;
                    Some(root)
                }
                Some(_) => Some(channel.to_owned()),
                // In no thread: it shows nowhere, so no draft is done.
                None => None,
            };
            if let Some(target) = draft_target {
                state.store.set_draft(&target, "", now_ms()?)?;
            }
        }
        self.emit(Change::Drafts);
        Ok(())
    }

    /// Keeps unsent text for a channel or a thread (its first message), on
    /// this device only. Blank text removes the draft.
    pub fn set_draft(&self, target: &str, text: &str) -> Result<()> {
        let kind = self.state().store.item_kind(target)?;
        if !matches!(kind, Some(ItemKind::Channel | ItemKind::Message)) {
            bail!("drafts belong to a channel or a thread");
        }
        self.state().store.set_draft(target, text, now_ms()?)?;
        self.emit(Change::Drafts);
        Ok(())
    }

    /// Your drafts, newest first. Drafts for something since deleted are left out.
    pub fn drafts(&self) -> Result<Vec<Draft>> {
        let state = self.state();
        let mut drafts = Vec::new();
        for (target, text, updated_ms) in state.store.drafts()? {
            let channel = match state.store.item_kind(&target)? {
                Some(ItemKind::Channel) => target.clone(),
                Some(ItemKind::Message) => match state.store.message(&target, &self.me())? {
                    Some(message) => message.channel,
                    None => continue,
                },
                _ => continue,
            };
            drafts.push(Draft {
                target,
                channel,
                text,
                updated_ms,
            });
        }
        Ok(drafts)
    }

    /// Whether new replies in the thread starting at `root` alert you.
    /// Personal: overrides following threads you started or joined.
    pub fn set_following(&self, root: &str, is_following: bool) -> Result<()> {
        self.require_item(root, ItemKind::Message)?;
        self.state().store.set_following(root, is_following)?;
        self.emit_messages_of(root)
    }

    /// Moves your read position in the thread starting at `root` up to
    /// `message`. Personal, like channel read positions.
    pub fn mark_thread_read(&self, root: &str, message: &str) -> Result<()> {
        self.state().store.mark_thread_read(root, message)?;
        self.emit_messages_of(root)
    }

    fn emit_messages_of(&self, message: &str) -> Result<()> {
        let channel = self
            .thread(message)?
            .first()
            .context("no such message")?
            .channel
            .clone();
        self.emit(Change::Messages { channel });
        Ok(())
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
        let text = checked_message_text(text)?;
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
        ensure!(
            clean_emoji(emoji) == emoji,
            "a reaction is one emoji (up to {MAX_EMOJI_CHARS} characters)"
        );
        self.write(Op::Reaction {
            message: message.to_owned(),
            emoji: emoji.to_owned(),
            active,
        })
    }

    pub fn create_page(&self, title: &str, parent: Option<EntityId>) -> Result<EntityId> {
        let title = checked_page_title(title)?;
        let id = new_owned_id(&self.me());
        self.write(Op::PageCreate {
            id: id.clone(),
            parent,
            title: title.to_owned(),
        })?;
        Ok(id)
    }

    pub fn rename_page(&self, page: &str, title: &str) -> Result<()> {
        let title = checked_page_title(title)?;
        self.write(Op::PageRename {
            id: page.to_owned(),
            title: title.to_owned(),
        })
    }

    /// Its sub-pages stay, moving to the top level. Owner or admin only.
    pub fn delete_page(&self, page: &str) -> Result<()> {
        self.require_item(page, ItemKind::Page)?;
        self.write(Op::Delete { id: page.to_owned() })
    }

    /// Edits and deletes name an existing item of the expected kind, so a
    /// mistaken id fails here instead of silently doing nothing.
    pub(crate) fn require_item(&self, id: &str, kind: ItemKind) -> Result<()> {
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
            name: checked_profile_name(profile.name)?,
            is_bot: profile.is_bot,
        })
    }

    /// Shares your profile details with everyone in the workspace,
    /// replacing what you shared before. Refused, not changed, if a field
    /// is too long or malformed.
    pub fn set_profile_details(&self, details: &ProfileDetails) -> Result<()> {
        self.write(Op::ProfileDetails {
            details: details.checked()?,
        })
    }
}

/// Your own name, refused rather than cut, so what you typed is what others see.
pub(crate) fn checked_profile_name(name: &str) -> Result<String> {
    let name = name.trim();
    ensure!(
        clean_display_name(name) == name,
        "a name has up to {MAX_PROFILE_NAME_CHARS} characters, without invisible ones"
    );
    Ok(name.to_owned())
}

pub(crate) fn checked_message_text(text: &str) -> Result<&str> {
    let text = text.trim();
    ensure!(!text.is_empty(), "message is empty");
    ensure!(
        text.chars().count() <= MAX_MESSAGE_CHARS,
        "a message has at most {MAX_MESSAGE_CHARS} characters"
    );
    Ok(text)
}

fn checked_page_title(title: &str) -> Result<&str> {
    let title = title.trim();
    ensure!(
        title.chars().count() <= MAX_PAGE_TITLE_CHARS,
        "a page title has at most {MAX_PAGE_TITLE_CHARS} characters"
    );
    ensure!(
        clean_page_title(title) == title,
        "a page title can't hold line breaks or invisible characters"
    );
    Ok(title)
}

fn too_large(op: &Op) -> String {
    match op {
        Op::DocUpdate { .. } => format!(
            "this change is too large to save (over {} MB); paste it in smaller parts",
            MAX_PAGE_UPDATE_BYTES / (1024 * 1024)
        ),
        _ => format!("this is too large to send (over {} KB)", MAX_OP_BYTES / 1024),
    }
}
