//! The core for the phone apps: a thin layer over `shy-core`'s `Workspaces`
//! and `Workspace` that UniFFI turns into Swift and Kotlin. Like the desktop's
//! Tauri commands, it maps calls one to one and holds no logic. Every call that
//! touches the core runs on this crate's own Tokio runtime, off the app's
//! threads.

mod app;
mod error;
mod keys;
mod runtime;
mod workspace;

#[cfg(test)]
mod tests;

pub use app::{BucketSettings, OpenFailure, ShyApp};
pub use error::ShyError;
pub use keys::{DeviceKeyStore, DeviceKeyStoreError};
pub use workspace::{ChangeListener, ShyWorkspace, Subscription};

uniffi::setup_scaffolding!();
