//! What a notification's quick actions do with the message it alerted
//! about: reply where it is, keep a reply that couldn't be sent, or mark
//! its conversation read.

use anyhow::{Context, Result};

use super::Workspace;
use crate::views::Change;

/// Where a message is read and answered: its thread if it's a reply in
/// one, otherwise its channel.
struct Conversation {
    channel: String,
    thread_root: Option<String>,
}

impl Conversation {
    fn draft_target(&self) -> &str {
        self.thread_root.as_deref().unwrap_or(&self.channel)
    }
}

impl Workspace {
    fn conversation_of(&self, message: &str) -> Result<Conversation> {
        let linked = self
            .linked_message(message)?
            .context("the message is no longer there")?;
        Ok(Conversation {
            channel: linked.message.channel,
            thread_root: linked.thread_root,
        })
    }

    /// Sends `text` where `message` is: into its thread if it's a reply,
    /// otherwise into its channel, as the composer there would. A draft
    /// there stays, in its place: it isn't what was sent.
    pub fn reply_to_alert(&self, message: &str, text: &str) -> Result<()> {
        let conversation = self.conversation_of(message)?;
        let target = conversation.draft_target().to_owned();
        let draft = self
            .drafts()?
            .into_iter()
            .find(|draft| draft.target == target && draft.editing.is_none());
        self.send_message(&conversation.channel, text, conversation.thread_root)?;
        if let Some(draft) = draft {
            self.state()
                .store
                .set_draft(&target, &draft.text, draft.updated_ms)?;
            self.emit(Change::Drafts);
        }
        Ok(())
    }

    /// Keeps a reply that couldn't be sent as the draft where it would
    /// have gone, after any draft already there.
    pub fn keep_unsent_reply(&self, message: &str, text: &str) -> Result<()> {
        let conversation = self.conversation_of(message)?;
        let target = conversation.draft_target();
        let kept = match self
            .drafts()?
            .into_iter()
            .find(|draft| draft.target == target && draft.editing.is_none())
        {
            Some(draft) if !draft.text.trim().is_empty() => format!("{}\n\n{text}", draft.text),
            _ => text.to_owned(),
        };
        self.set_draft(target, &kept)
    }

    /// Reads the conversation `message` is in up to that message, never
    /// past it, so nothing that came after it is marked read unseen.
    pub fn mark_alert_read(&self, message: &str) -> Result<()> {
        let conversation = self.conversation_of(message)?;
        match conversation.thread_root {
            Some(root) => self.mark_thread_read(&root, message),
            None => self.mark_read(&conversation.channel, message),
        }
    }
}
