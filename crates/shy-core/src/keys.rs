//! The device key: one random secret per data directory, kept outside it, in
//! the OS keychain or a key file the user chooses. Every local database is
//! encrypted with a key derived from it, so the data directory alone reveals
//! nothing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use zeroize::Zeroizing;

use crate::config::write_private_file;

const KEYCHAIN_SERVICE: &str = "shyspace";
const KEY_LEN: usize = 32;

/// Where the device key lives.
#[derive(Debug, Clone)]
pub enum KeyStorage {
    /// The OS keychain (Secret Service, macOS Keychain, Windows Credential Manager).
    Keychain,
    /// A file only its owner can read. For servers without a keychain; keep it
    /// outside the data directory, or it protects nothing.
    File(PathBuf),
}

pub struct DeviceKey(Zeroizing<[u8; KEY_LEN]>);

impl DeviceKey {
    pub(crate) fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub(crate) fn to_bytes(&self) -> Zeroizing<[u8; KEY_LEN]> {
        self.0.clone()
    }

    /// The SQLCipher key for one database, as a raw-key literal.
    pub fn database_key(&self, database: &str) -> Zeroizing<String> {
        let context = format!("shyspace local database {database} v1");
        let key = Zeroizing::new(blake3::derive_key(&context, self.0.as_slice()));
        Zeroizing::new(format!("x'{}'", hex::encode(key.as_slice())))
    }
}

impl KeyStorage {
    /// The key for `dir`, creating one the first time.
    pub async fn load_or_create(&self, dir: &Path) -> Result<DeviceKey> {
        match self.load(dir).await {
            Ok(key) => Ok(key),
            Err(error) if error.downcast_ref::<MissingKey>().is_some() => self.create(dir).await,
            Err(error) => Err(error),
        }
    }

    pub async fn load(&self, dir: &Path) -> Result<DeviceKey> {
        let bytes = match self {
            Self::Keychain => {
                let account = keychain_account(dir)?;
                tokio::task::spawn_blocking(move || read_keychain(&account)).await??
            }
            Self::File(path) => match std::fs::read(path) {
                Ok(bytes) => Zeroizing::new(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(MissingKey.into()),
                Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
            },
        };
        let key: [u8; KEY_LEN] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("the stored device key is corrupt"))?;
        Ok(DeviceKey(Zeroizing::new(key)))
    }

    /// Deletes the key for `dir`. Its data becomes unreadable for good.
    pub(crate) async fn forget(&self, dir: &Path) -> Result<()> {
        match self {
            Self::Keychain => {
                let account = keychain_account(dir)?;
                tokio::task::spawn_blocking(move || match keychain_entry(&account)?.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                    Err(error) => Err(anyhow::Error::new(error).context("deleting the device key")),
                })
                .await?
            }
            Self::File(path) => match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error).with_context(|| format!("deleting {}", path.display())),
            },
        }
    }

    async fn create(&self, dir: &Path) -> Result<DeviceKey> {
        let key = DeviceKey::from_bytes(rand::random::<[u8; KEY_LEN]>());
        self.save(dir, &key).await?;
        Ok(key)
    }

    /// Stores an existing key for `dir`, as when restoring onto a new device.
    pub(crate) async fn save(&self, dir: &Path, device_key: &DeviceKey) -> Result<()> {
        let key = device_key.to_bytes();
        match self {
            Self::Keychain => {
                let account = keychain_account(dir)?;
                let secret = key.clone();
                tokio::task::spawn_blocking(move || write_keychain(&account, secret.as_slice())).await??;
            }
            Self::File(path) => {
                if path.exists() {
                    bail!("{} already exists; choose a new key file", path.display());
                }
                write_private_file(path, key.as_slice())?;
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct MissingKey;

impl std::fmt::Display for MissingKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("no device key found for this data directory")
    }
}

impl std::error::Error for MissingKey {}

/// One keychain entry per data directory, so several workspaces (or test
/// members) on one machine don't share a key.
fn keychain_account(dir: &Path) -> Result<String> {
    let canonical = dir
        .canonicalize()
        .with_context(|| format!("resolving {}", dir.display()))?;
    let digest = blake3::hash(canonical.as_os_str().as_encoded_bytes());
    Ok(format!("device-key-{}", &digest.to_hex()[..32]))
}

fn keychain_entry(account: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, account).map_err(|error| match error {
        keyring::Error::NoDefaultStore => {
            anyhow::anyhow!("no system keychain is available; on a server, use a key file (--key-file)")
        }
        other => anyhow::Error::new(other).context("opening the system keychain"),
    })
}

fn read_keychain(account: &str) -> Result<Zeroizing<Vec<u8>>> {
    match keychain_entry(account)?.get_secret() {
        Ok(secret) => Ok(Zeroizing::new(secret)),
        Err(keyring::Error::NoEntry) => Err(MissingKey.into()),
        Err(error) => Err(anyhow::Error::new(error).context("reading the device key from the keychain")),
    }
}

fn write_keychain(account: &str, secret: &[u8]) -> Result<()> {
    let entry = keychain_entry(account)?;
    if entry.get_secret().is_ok() {
        bail!("a device key for this data directory already exists in the keychain");
    }
    entry
        .set_secret(secret)
        .context("saving the device key to the keychain")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn file_keys_are_created_once_then_reused() {
        let dir = tempfile::tempdir().unwrap();
        let storage = KeyStorage::File(dir.path().join("device.key"));
        assert!(storage.load(dir.path()).await.is_err());
        let created = storage.load_or_create(dir.path()).await.unwrap();
        let loaded = storage.load_or_create(dir.path()).await.unwrap();
        assert_eq!(*created.database_key("shy"), *loaded.database_key("shy"));
        assert_ne!(*created.database_key("shy"), *created.database_key("mls"));
    }
}
