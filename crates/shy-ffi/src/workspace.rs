//! One open workspace: its channels and messages, and its changes.

use std::sync::Arc;

use shy_core::{
    Workspace,
    views::{Change, Channel, Message, MessageCursor},
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
