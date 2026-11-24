//! The shyspace engine. Clients (desktop app, CLI, agents) are thin layers
//! over [`Workspace`]. See `docs/architecture.md` for how the parts fit.

pub mod config;
pub mod crypto;
mod device_settings;
pub mod files;
pub mod huddle;
pub mod identity;
pub mod keys;
pub mod links;
mod markdown;
pub mod membership;
pub mod mentions;
pub mod ops;
pub mod pages;
pub mod private;
pub mod profile;
pub mod search;
pub mod storage;
pub mod sync;
pub mod transport;
pub mod views;
pub mod workspace;
pub mod workspaces;

#[cfg(test)]
mod tests;

pub use keys::KeyStorage;
pub use pages::Edit;
pub use transport::{bucket::BucketConfig, gossip::ExtraProtocols};
pub use workspace::{DataDir, JoinCancel, Networking, Profile, Workspace, WorkspaceOptions};
