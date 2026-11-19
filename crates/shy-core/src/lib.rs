//! The shyspace engine. Clients (desktop app, CLI, agents) are thin layers
//! over [`Workspace`]. See `docs/architecture.md` for how the parts fit.

pub mod config;
pub mod crypto;
pub mod identity;
pub mod keys;
pub mod membership;
pub mod mentions;
pub mod ops;
pub mod pages;
pub mod storage;
pub mod sync;
pub mod transport;
pub mod views;
pub mod workspace;

#[cfg(test)]
mod tests;

pub use keys::KeyStorage;
pub use pages::Edit;
pub use transport::bucket::BucketConfig;
pub use workspace::{DataDir, Networking, Profile, Workspace};
