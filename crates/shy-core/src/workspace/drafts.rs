//! Drafts: unsent text per conversation, and unsaved edits of your own
//! messages, kept on this device only.

use anyhow::{Context, Result, bail, ensure};

use super::Workspace;
use crate::{
    ops::now_ms,
    storage::{ItemKind, Store},
    views::{Change, Draft, EditDraft},
};

/// Edits share the drafts table with conversation drafts. Their key names
/// the conversation the edit was made in, so it resumes there and keeps its
/// channel if the message is deleted. Every id starts with hex or a private
/// kind (`dm:`, `p:`), so no conversation's own key starts like an edit's;
/// private ids hold `:` and `/`, hence `|` between the parts.
const EDIT_KEY_PREFIX: &str = "edit|";
const EDIT_KEY_SEPARATOR: char = '|';

fn edit_key(conversation: &str, message: &str) -> String {
    format!("{EDIT_KEY_PREFIX}{conversation}{EDIT_KEY_SEPARATOR}{message}")
}

/// The conversation and message an edit's key names. Split at the last
/// separator: the message is yours, its id made here without one, while
/// another member's conversation id isn't checked for it.
fn parse_edit_key(key: &str) -> Option<(&str, &str)> {
    key.strip_prefix(EDIT_KEY_PREFIX)?.rsplit_once(EDIT_KEY_SEPARATOR)
}

impl Workspace {
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

    /// Keeps the unsaved text of an edit to your `message`, made in
    /// `conversation` (a channel, or a thread's first message), on this
    /// device only. Blank text removes it, even once the message is gone.
    pub fn set_edit_draft(&self, conversation: &str, message: &str, text: &str) -> Result<()> {
        let state = self.state();
        if !text.trim().is_empty() {
            let channel = conversation_channel(&state.store, conversation, self)?
                .context("edits are kept for a channel or a thread")?;
            let edited = state
                .store
                .message(message, &self.my_person())?
                .context("no such message here")?;
            let edit = crate::ops::Op::MessageEdit {
                id: message.to_owned(),
                text: String::new(),
            };
            ensure!(
                self.may(&state.store, state.store.newest_epoch()?, &edit)?,
                "you can only edit your own messages"
            );
            ensure!(
                edited.channel == channel,
                "the message isn't in that conversation"
            );
        }
        state
            .store
            .set_draft(&edit_key(conversation, message), text, now_ms()?)?;
        drop(state);
        self.emit(Change::Drafts);
        Ok(())
    }

    /// Moves the text of an edit whose message was deleted into the draft of
    /// the conversation it was made in, after any draft already there, so
    /// it isn't lost.
    pub fn keep_edit_of_deleted_message(&self, conversation: &str, message: &str) -> Result<()> {
        let state = self.state();
        ensure!(
            state.store.item_kind(message)? != Some(ItemKind::Message),
            "the message is still there to edit"
        );
        let key = edit_key(conversation, message);
        let stored = state.store.drafts()?;
        let Some((_, edit, _)) = stored.iter().find(|(target, _, _)| *target == key) else {
            return Ok(());
        };
        let kept = match stored.iter().find(|(target, _, _)| target == conversation) {
            Some((_, draft, _)) if !draft.trim().is_empty() => format!("{draft}\n\n{edit}"),
            _ => edit.clone(),
        };
        let now = now_ms()?;
        state.store.atomically(|store| {
            store.set_draft(conversation, &kept, now)?;
            store.set_draft(&key, "", now)
        })?;
        drop(state);
        self.emit(Change::Drafts);
        Ok(())
    }

    /// Your drafts and unsaved edits, newest first. Drafts for a
    /// conversation since deleted are left out; an edit whose message was
    /// deleted stays, marked so, until its text is kept elsewhere.
    pub fn drafts(&self) -> Result<Vec<Draft>> {
        let state = self.state();
        let mut drafts = Vec::new();
        for (key, text, updated_ms) in state.store.drafts()? {
            let (target, editing) = match parse_edit_key(&key) {
                Some((conversation, message)) => (
                    conversation.to_owned(),
                    Some(EditDraft {
                        message: message.to_owned(),
                        is_deleted: state.store.item_kind(message)? != Some(ItemKind::Message),
                    }),
                ),
                None => (key, None),
            };
            let Some(channel) = conversation_channel(&state.store, &target, self)? else {
                continue;
            };
            drafts.push(Draft {
                target,
                channel,
                text,
                updated_ms,
                editing,
            });
        }
        Ok(drafts)
    }
}

/// The channel `conversation` (a channel, or a thread's first message) is
/// in; None once it's gone.
fn conversation_channel(store: &Store, conversation: &str, workspace: &Workspace) -> Result<Option<String>> {
    Ok(match store.item_kind(conversation)? {
        Some(ItemKind::Channel) => Some(conversation.to_owned()),
        Some(ItemKind::Message) => store
            .message(conversation, &workspace.my_person())?
            .map(|message| message.channel),
        _ => None,
    })
}
