//! The app's keystore (iOS Keychain, Android Keystore), offered to the core
//! as its place for device keys.

use std::sync::Arc;

use shy_core::keys::HostKeyStore;
use zeroize::Zeroizing;

/// The app's secret store for device keys, one secret per account. Calls may
/// block. Breaking this contract can make a workspace's data unreadable for
/// good:
/// - `read` returns `None` only when the account has no secret; anything else
///   (locked before first unlock, access denied, can't decrypt) is an error.
/// - `create` refuses an account that already has a secret, atomically, and
///   returns only once the secret is stored durably.
/// - Secrets are never logged, cached, or put in error text.
#[uniffi::export(with_foreign)]
pub trait DeviceKeyStore: Send + Sync {
    fn read(&self, account: String) -> Result<Option<Vec<u8>>, DeviceKeyStoreError>;
    fn create(&self, account: String, secret: Vec<u8>) -> Result<(), DeviceKeyStoreError>;
    /// Succeeds if the account has no secret.
    fn delete(&self, account: String) -> Result<(), DeviceKeyStoreError>;
}

#[derive(Debug, uniffi::Error)]
pub enum DeviceKeyStoreError {
    Failed { reason: String },
}

impl std::fmt::Display for DeviceKeyStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed { reason } => formatter.write_str(reason),
        }
    }
}

impl std::error::Error for DeviceKeyStoreError {}

impl From<uniffi::UnexpectedUniFFICallbackError> for DeviceKeyStoreError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Failed { reason: error.reason }
    }
}

pub(crate) struct AppKeyStore(pub(crate) Arc<dyn DeviceKeyStore>);

impl HostKeyStore for AppKeyStore {
    fn read(&self, account: &str) -> anyhow::Result<Option<Zeroizing<Vec<u8>>>> {
        Ok(self.0.read(account.to_owned())?.map(Zeroizing::new))
    }

    fn create(&self, account: &str, secret: &[u8]) -> anyhow::Result<()> {
        Ok(self.0.create(account.to_owned(), secret.to_vec())?)
    }

    fn delete(&self, account: &str) -> anyhow::Result<()> {
        Ok(self.0.delete(account.to_owned())?)
    }
}
