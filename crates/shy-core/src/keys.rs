//! The device key: one random secret per data directory, kept outside it, in
//! the OS keychain, a key file the user chooses, or a store the hosting app
//! provides. Every local database is encrypted with a key derived from it, so
//! the data directory alone reveals nothing.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail, ensure};
use zeroize::Zeroizing;

use crate::config::write_private_file;

const KEYCHAIN_SERVICE: &str = "shyspace";
const KEY_LEN: usize = 32;

/// A secret store run by the app hosting the core, such as the iOS Keychain
/// or a file sealed with an Android Keystore key. Calls may block.
///
/// The core relies on this contract; breaking it can make a workspace's data
/// unreadable for good:
/// - `read` answers `None` only when the account has no secret. Anything else
///   (locked before first unlock, access denied, can't decrypt) is an error.
/// - `create` refuses an account that already has a secret, atomically, and
///   returns only once the secret is stored durably.
/// - Secrets are never logged, cached outside the store, or put in error
///   text; errors are logged.
pub trait HostKeyStore: Send + Sync {
    /// The secret saved under `account`, or `None` if there is none.
    fn read(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>>;
    /// Saves a secret under an `account` that has none.
    fn create(&self, account: &str, secret: &[u8]) -> Result<()>;
    /// Deletes the secret under `account`; succeeds if there was none.
    fn delete(&self, account: &str) -> Result<()>;
}

/// Where the device key lives.
#[derive(Clone)]
pub enum KeyStorage {
    /// The OS keychain (Secret Service, macOS Keychain, Windows Credential Manager).
    Keychain,
    /// A file only its owner can read. For servers without a keychain; keep it
    /// outside the data directory, or it protects nothing.
    File(PathBuf),
    /// The hosting app's store, under one account per data directory.
    Host {
        store: Arc<dyn HostKeyStore>,
        account: String,
    },
}

/// Shows no more than where the key lives, so a `{:?}` never reaches a
/// store that might print what it holds.
impl std::fmt::Debug for KeyStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keychain => formatter.write_str("Keychain"),
            Self::File(path) => formatter.debug_tuple("File").field(path).finish(),
            Self::Host { account, .. } => formatter.debug_struct("Host").field("account", account).finish(),
        }
    }
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
    /// Where the key for a second folder beside the one this storage is
    /// for lives: the keychain names each folder's key already; a key file
    /// or the app's store gets a second name, so deleting one key never
    /// deletes the other.
    pub(crate) fn beside(&self, suffix: &str) -> Self {
        match self {
            Self::Keychain => Self::Keychain,
            Self::File(path) => {
                let mut beside = path.clone().into_os_string();
                beside.push(suffix);
                Self::File(beside.into())
            }
            Self::Host { store, account } => Self::Host {
                store: store.clone(),
                account: format!("{account}{suffix}"),
            },
        }
    }

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
            Self::Host { store, account } => {
                let (store, account) = (store.clone(), account.clone());
                tokio::task::spawn_blocking(move || store.read(&account))
                    .await?
                    .context("reading the device key from the app's keystore")?
                    .ok_or(MissingKey)?
            }
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
            Self::Host { store, account } => {
                let (store, account) = (store.clone(), account.clone());
                tokio::task::spawn_blocking(move || store.delete(&account))
                    .await?
                    .context("deleting the device key from the app's keystore")
            }
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
            Self::Host { store, account } => {
                let (store, account) = (store.clone(), account.clone());
                tokio::task::spawn_blocking(move || {
                    if store.read(&account)?.is_some() {
                        bail!("a device key for this data directory already exists in the app's keystore");
                    }
                    store.create(&account, key.as_slice())?;
                    // A store that reports success without storing would lose
                    // the key at the next start, with the data written under it.
                    let stored = store.read(&account)?;
                    ensure!(
                        stored.as_deref().map(Vec::as_slice) == Some(key.as_slice()),
                        "the app's keystore didn't keep the device key it was given"
                    );
                    Ok(())
                })
                .await?
                .context("saving the device key to the app's keystore")?;
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
/// members) on one machine don't share a key. A directory not made yet is
/// named through its parent, as it will resolve once made.
fn keychain_account(dir: &Path) -> Result<String> {
    let canonical = match dir.canonicalize() {
        Ok(canonical) => canonical,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) else {
                return Err(error).with_context(|| format!("resolving {}", dir.display()));
            };
            parent
                .canonicalize()
                .with_context(|| format!("resolving {}", parent.display()))?
                .join(name)
        }
        Err(error) => return Err(error).with_context(|| format!("resolving {}", dir.display())),
    };
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
    write_if_absent(|| entry.get_secret(), || entry.set_secret(secret))
}

/// Writes only when the keychain says there's no entry: any other answer to
/// the read (locked, denied) might hide a key that writing would replace.
fn write_if_absent(
    read: impl FnOnce() -> keyring::Result<Vec<u8>>,
    write: impl FnOnce() -> keyring::Result<()>,
) -> Result<()> {
    match read() {
        Ok(_) => bail!("a device key for this data directory already exists in the keychain"),
        Err(keyring::Error::NoEntry) => write().context("saving the device key to the keychain"),
        Err(error) => Err(anyhow::Error::new(error).context("checking the keychain for a device key")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keychain_is_written_only_when_it_has_no_entry() {
        let mut written = false;
        write_if_absent(
            || Err(keyring::Error::NoEntry),
            || {
                written = true;
                Ok(())
            },
        )
        .unwrap();
        assert!(written);
    }

    #[test]
    fn an_existing_keychain_entry_is_never_replaced() {
        let result = write_if_absent(|| Ok(vec![1; KEY_LEN]), || panic!("must not write"));
        assert!(result.is_err());
    }

    #[test]
    fn a_keychain_that_fails_to_answer_is_an_error_and_is_not_written() {
        let result = write_if_absent(
            || Err(keyring::Error::Invalid("keychain".into(), "locked".into())),
            || panic!("must not write"),
        );
        assert!(format!("{:#}", result.err().unwrap()).contains("checking the keychain"));
    }

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

    #[derive(Default, Clone, Copy, PartialEq)]
    enum Behaviour {
        #[default]
        Honest,
        Locked,
        /// Reports a save as done without keeping it.
        Forgetful,
        Panicking,
    }

    #[derive(Default)]
    struct MemoryStore {
        secrets: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
        behaviour: Behaviour,
    }

    impl MemoryStore {
        fn behaving(behaviour: Behaviour) -> Self {
            Self {
                behaviour,
                ..Self::default()
            }
        }

        fn put(&self, account: &str, secret: &[u8]) {
            self.secrets
                .lock()
                .unwrap()
                .insert(account.to_owned(), secret.to_vec());
        }
    }

    impl HostKeyStore for MemoryStore {
        fn read(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
            match self.behaviour {
                Behaviour::Locked => bail!("the keystore is locked"),
                Behaviour::Panicking => panic!("the keystore crashed"),
                Behaviour::Honest | Behaviour::Forgetful => Ok(self
                    .secrets
                    .lock()
                    .unwrap()
                    .get(account)
                    .cloned()
                    .map(Zeroizing::new)),
            }
        }

        fn create(&self, account: &str, secret: &[u8]) -> Result<()> {
            if self.behaviour == Behaviour::Forgetful {
                return Ok(());
            }
            let mut secrets = self.secrets.lock().unwrap();
            ensure!(!secrets.contains_key(account), "{account} already has a secret");
            secrets.insert(account.to_owned(), secret.to_vec());
            Ok(())
        }

        fn delete(&self, account: &str) -> Result<()> {
            self.secrets.lock().unwrap().remove(account);
            Ok(())
        }
    }

    fn host_storage(store: &Arc<MemoryStore>) -> KeyStorage {
        KeyStorage::Host {
            store: store.clone(),
            account: "device-key-abc".into(),
        }
    }

    #[tokio::test]
    async fn host_keys_are_created_once_then_reused() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MemoryStore::default());
        let storage = host_storage(&store);
        assert!(storage.load(dir.path()).await.is_err());
        let created = storage.load_or_create(dir.path()).await.unwrap();
        let loaded = storage.load_or_create(dir.path()).await.unwrap();
        assert_eq!(*created.database_key("shy"), *loaded.database_key("shy"));
        assert_eq!(store.secrets.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_host_key_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MemoryStore::default());
        let storage = host_storage(&store);
        let first = storage.load_or_create(dir.path()).await.unwrap();
        let other = DeviceKey::from_bytes([7; KEY_LEN]);
        assert!(storage.save(dir.path(), &other).await.is_err());
        let loaded = storage.load(dir.path()).await.unwrap();
        assert_eq!(*first.database_key("shy"), *loaded.database_key("shy"));
    }

    #[tokio::test]
    async fn a_forgotten_host_key_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MemoryStore::default());
        let storage = host_storage(&store);
        storage.load_or_create(dir.path()).await.unwrap();
        storage.forget(dir.path()).await.unwrap();
        assert!(store.secrets.lock().unwrap().is_empty());
        storage.forget(dir.path()).await.unwrap();
    }

    #[tokio::test]
    async fn a_key_beside_another_is_its_own_and_forgetting_it_keeps_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MemoryStore::default());
        let file = dir.path().join("first.key");
        for storage in [host_storage(&store), KeyStorage::File(file)] {
            let first = storage.load_or_create(dir.path()).await.unwrap();
            let beside = storage.beside(".linking");
            let second = beside.load_or_create(dir.path()).await.unwrap();
            assert_ne!(*first.database_key("shy"), *second.database_key("shy"));

            beside.forget(dir.path()).await.unwrap();

            let kept = storage.load(dir.path()).await.unwrap();
            assert_eq!(*first.database_key("shy"), *kept.database_key("shy"));
        }
    }

    #[tokio::test]
    async fn a_corrupt_host_key_is_refused_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MemoryStore::default());
        store.put("device-key-abc", b"too short");
        let storage = host_storage(&store);
        assert!(storage.load_or_create(dir.path()).await.is_err());
        assert_eq!(
            store.secrets.lock().unwrap().get("device-key-abc").unwrap(),
            b"too short"
        );
    }

    #[tokio::test]
    async fn a_failing_host_store_is_an_error_not_a_new_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MemoryStore::behaving(Behaviour::Locked));
        let error = host_storage(&store)
            .load_or_create(dir.path())
            .await
            .err()
            .unwrap();
        assert!(format!("{error:#}").contains("the keystore is locked"));
        assert!(store.secrets.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_host_store_that_doesnt_keep_the_key_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MemoryStore::behaving(Behaviour::Forgetful));
        let error = host_storage(&store)
            .load_or_create(dir.path())
            .await
            .err()
            .unwrap();
        assert!(format!("{error:#}").contains("didn't keep the device key"));
    }

    #[tokio::test]
    async fn a_crashing_host_store_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MemoryStore::behaving(Behaviour::Panicking));
        assert!(host_storage(&store).load_or_create(dir.path()).await.is_err());
    }

    #[test]
    fn debug_output_names_the_account_and_nothing_the_store_holds() {
        let store = Arc::new(MemoryStore::default());
        store.put("device-key-abc", b"top secret bytes");
        let shown = format!("{:?}", host_storage(&store));
        assert_eq!(shown, r#"Host { account: "device-key-abc" }"#);
    }

    #[test]
    fn a_folder_is_named_the_same_before_and_after_it_is_made() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("real")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.path().join("real"), root.path().join("link")).unwrap();
        let dir = root.path().join("link/workspaces/abc");
        std::fs::create_dir_all(root.path().join("link/workspaces")).unwrap();
        let before = keychain_account(&dir).unwrap();
        std::fs::create_dir(&dir).unwrap();
        assert_eq!(before, keychain_account(&dir).unwrap());
        assert_eq!(
            before,
            keychain_account(&root.path().join("real/workspaces/abc")).unwrap()
        );
    }
}
