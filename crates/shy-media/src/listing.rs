//! Asking the system for its audio devices, one listing at a time. A sound
//! server can stop answering: a listing that hangs is waited out instead of
//! being joined by more (each holds a thread), and it never holds up the
//! call's loop, so muting and switching devices still act at once.

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use shy_core::huddle::media::DeviceList;
use tokio::{task::JoinHandle, time::Instant};
use tracing::warn;

use crate::backend::Backend;

/// How long the system may take to list its devices.
const LISTING_LIMIT: Duration = Duration::from_secs(5);

pub(crate) struct DeviceListing<B: Backend> {
    backend: Arc<B>,
    running: Option<Running>,
}

struct Running {
    task: JoinHandle<Result<DeviceList>>,
    /// When it counts as too slow; `None` once it has, while it's waited out.
    deadline: Option<Instant>,
}

impl<B: Backend> DeviceListing<B> {
    pub(crate) fn new(backend: Arc<B>) -> Self {
        Self {
            backend,
            running: None,
        }
    }

    /// Starts a listing, unless one is still running.
    pub(crate) fn start(&mut self) {
        if self.running.is_some() {
            return;
        }
        let backend = self.backend.clone();
        self.running = Some(Running {
            task: tokio::task::spawn_blocking(move || backend.list_devices()),
            deadline: Some(Instant::now() + LISTING_LIMIT),
        });
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// Lists the devices and waits for the answer, within the time limit;
    /// while an earlier listing still hangs, gives up at once.
    pub(crate) async fn list_now(&mut self) -> Option<DeviceList> {
        if self
            .running
            .as_ref()
            .is_some_and(|running| running.deadline.is_none())
        {
            return None;
        }
        self.start();
        self.finished().await
    }

    /// The running listing's answer: `None` if it failed, and also when it
    /// passes the time limit, after which it's waited out and its late answer
    /// still comes here. Safe to drop unfinished, as in a `select!`.
    pub(crate) async fn finished(&mut self) -> Option<DeviceList> {
        let Some(running) = self.running.as_mut() else {
            return std::future::pending().await;
        };
        let joined = match running.deadline {
            Some(deadline) => match tokio::time::timeout_at(deadline, &mut running.task).await {
                Ok(joined) => joined,
                Err(_) => {
                    running.deadline = None;
                    warn!("listing the audio devices takes too long; waiting for it before listing again");
                    return None;
                }
            },
            None => (&mut running.task).await,
        };
        self.running = None;
        match joined {
            Ok(Ok(listed)) => Some(listed),
            Ok(Err(error)) => {
                warn!(error = %error, "the audio devices can't be listed");
                None
            }
            Err(crashed) => {
                warn!(error = %crashed, "listing the audio devices crashed");
                None
            }
        }
    }
}
