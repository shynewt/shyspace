//! The one Tokio runtime the core runs on. The core spawns tasks (sync,
//! gossip), so every call must enter a runtime, and the app's own threads
//! have none.

use std::{future::Future, sync::LazyLock};

use tokio::runtime::{Builder, Runtime};

use crate::ShyError;

static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    Builder::new_multi_thread()
        .enable_all()
        .thread_name("shy-core")
        .build()
        .expect("starting the core's threads only fails when the system is out of resources")
});

pub(crate) fn handle() -> tokio::runtime::Handle {
    RUNTIME.handle().clone()
}

/// Runs `future` on the runtime.
pub(crate) async fn run<T: Send + 'static>(
    future: impl Future<Output = anyhow::Result<T>> + Send + 'static,
) -> Result<T, ShyError> {
    RUNTIME
        .spawn(future)
        .await
        .map_err(|error| anyhow::anyhow!("the core's task stopped: {error}"))?
        .map_err(ShyError::from)
}

/// Runs `work`, which may block (database reads and writes), on the runtime's
/// blocking threads.
pub(crate) async fn run_blocking<T: Send + 'static>(
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, ShyError> {
    RUNTIME
        .spawn_blocking(work)
        .await
        .map_err(|error| anyhow::anyhow!("the core's task stopped: {error}"))?
        .map_err(ShyError::from)
}
