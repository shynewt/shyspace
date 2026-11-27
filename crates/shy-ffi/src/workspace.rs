//! One open workspace: its channels and messages, and its changes.

use std::sync::Arc;

use shy_core::{
    Workspace,
    views::{Change, Channel, LinkedMessage, Member, Message, MessageCursor},
};
use tokio::{sync::broadcast::error::RecvError, task::AbortHandle};

use crate::{
    ShyError,
    runtime::{handle, run_blocking},
};

#[derive(uniffi::Object)]
pub struct ShyWorkspace {
    inner: Workspace,
}

/// Told of every change in a workspace, on one of the core's threads. It must
/// return at once (hand the change to the app's own thread or queue), or it
/// holds up the core.
#[uniffi::export(with_foreign)]
pub trait ChangeListener: Send + Sync {
    fn on_change(&self, change: Change);
}

/// Changes reach the listener until this is cancelled or dropped.
#[derive(uniffi::Object)]
pub struct Subscription {
    task: AbortHandle,
}

impl ShyWorkspace {
    pub(crate) fn new(inner: Workspace) -> Arc<Self> {
        Arc::new(Self { inner })
    }
}

#[uniffi::export]
impl ShyWorkspace {
    pub fn workspace_id(&self) -> String {
        self.inner.workspace_id().to_owned()
    }

    pub fn name(&self) -> String {
        self.inner.name().to_owned()
    }

    pub async fn channels(&self) -> Result<Vec<Channel>, ShyError> {
        let workspace = self.inner.clone();
        run_blocking(move || workspace.channels()).await
    }

    /// The latest `limit` messages in `channel`, or with `before`, the ones
    /// just older.
    pub async fn messages(
        &self,
        channel: String,
        before: Option<MessageCursor>,
        limit: u32,
    ) -> Result<Vec<Message>, ShyError> {
        let workspace = self.inner.clone();
        run_blocking(move || workspace.messages(&channel, before.as_ref(), limit)).await
    }

    /// Sends a message, or with `reply_to`, a reply in that message's thread.
    /// Returns the new message's id.
    pub async fn send_message(
        &self,
        channel: String,
        text: String,
        reply_to: Option<String>,
    ) -> Result<String, ShyError> {
        let workspace = self.inner.clone();
        run_blocking(move || workspace.send_message(&channel, &text, reply_to)).await
    }

    /// What a reader sees of each of `texts` as a message's body: app links
    /// named, no markdown marks, lines and list items kept.
    pub async fn plain_texts(&self, texts: Vec<String>) -> Result<Vec<String>, ShyError> {
        let workspace = self.inner.clone();
        run_blocking(move || workspace.plain_texts(&texts)).await
    }

    /// Everyone in the workspace, people once each: names for direct messages
    /// and mentions, admins and agents marked.
    pub async fn members(&self) -> Result<Vec<Member>, ShyError> {
        let workspace = self.inner.clone();
        run_blocking(move || workspace.members()).await
    }

    /// Adds your `emoji` to a message, or with `active` false, takes it away.
    pub async fn react(&self, message: String, emoji: String, active: bool) -> Result<(), ShyError> {
        let workspace = self.inner.clone();
        run_blocking(move || workspace.react(&message, &emoji, active)).await
    }

    /// Replaces the text of a message of yours.
    pub async fn edit_message(&self, message: String, text: String) -> Result<(), ShyError> {
        let workspace = self.inner.clone();
        run_blocking(move || workspace.edit_message(&message, &text)).await
    }

    /// Deletes a message of yours (or anyone's, for the admin).
    pub async fn delete_message(&self, message: String) -> Result<(), ShyError> {
        let workspace = self.inner.clone();
        run_blocking(move || workspace.delete_message(&message)).await
    }

    /// The thread starting at `root`: its first message, then every reply.
    pub async fn thread(&self, root: String) -> Result<Vec<Message>, ShyError> {
        let workspace = self.inner.clone();
        run_blocking(move || workspace.thread(&root)).await
    }

    /// The message `id`, with the first message of its thread if it's a reply; none once deleted.
    pub async fn linked_message(&self, id: String) -> Result<Option<LinkedMessage>, ShyError> {
        let workspace = self.inner.clone();
        run_blocking(move || workspace.linked_message(&id)).await
    }

    /// Moves your read position in the thread starting at `root` up to `message`.
    pub async fn mark_thread_read(&self, root: String, message: String) -> Result<(), ShyError> {
        let workspace = self.inner.clone();
        run_blocking(move || workspace.mark_thread_read(&root, &message)).await
    }

    /// Moves your read position in `channel` up to `message`, on this device only.
    pub async fn mark_read(&self, channel: String, message: String) -> Result<(), ShyError> {
        let workspace = self.inner.clone();
        run_blocking(move || workspace.mark_read(&channel, &message)).await
    }

    /// Tells `listener` of every change from now on. Missed changes (the
    /// listener fell behind) arrive as one `Change::Sync`: reload everything.
    pub fn subscribe(&self, listener: Arc<dyn ChangeListener>) -> Arc<Subscription> {
        let mut changes = self.inner.subscribe();
        let task = handle().spawn(async move {
            loop {
                let change = match changes.recv().await {
                    Ok(change) => change,
                    Err(RecvError::Lagged(_)) => Change::Sync,
                    Err(RecvError::Closed) => break,
                };
                listener.on_change(change);
            }
        });
        Arc::new(Subscription {
            task: task.abort_handle(),
        })
    }
}

#[uniffi::export]
impl Subscription {
    pub fn cancel(&self) {
        self.task.abort();
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.task.abort();
    }
}
