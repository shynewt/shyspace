//! Sealed file pieces kept on this device, so a file opened once opens again
//! without the network. Pieces stay encrypted here: their keys live only in
//! the encrypted database, so a copied data folder still reveals nothing.
//! Blobs are kept by author and id: ids are public, so anyone could reuse
//! one under their own prefix.

use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

use anyhow::{Context, Result, ensure};
use iroh::PublicKey;

use super::blob::is_blob_id;
use crate::views::member_id;

/// When the blob was last used; folders' own times don't change reliably
/// on every system.
const USED_MARKER: &str = "used";

pub struct BlobCache {
    root: PathBuf,
    /// Beyond this many bytes, the blobs opened longest ago are dropped.
    /// `None` keeps everything (profile pictures).
    limit_bytes: Option<u64>,
    size: CacheSize,
}

/// How many bytes a cache holds: measured once, then kept up to date by every
/// change made through a [`BlobCache`] sharing it, so trimming needn't read
/// every folder each time. Unknown until first needed, and again after a
/// change it couldn't count.
#[derive(Clone, Default)]
pub struct CacheSize(Arc<Mutex<KeptSize>>);

#[derive(Default)]
struct KeptSize {
    bytes: Option<u64>,
    /// Counts changes, so a count taken while others wrote isn't trusted.
    changes: u64,
}

impl CacheSize {
    fn known(&self) -> Option<u64> {
        self.lock().bytes
    }

    fn changes(&self) -> u64 {
        self.lock().changes
    }

    fn set(&self, bytes: Option<u64>) {
        self.lock().bytes = bytes;
    }

    /// Takes a full count as the size, unless the cache changed since
    /// `changes_before`, when the count began; then the size is measured
    /// again next time.
    fn counted(&self, bytes: u64, changes_before: u64) {
        let mut kept = self.lock();
        kept.bytes = (kept.changes == changes_before).then_some(bytes);
    }

    /// Adds `added` and takes away `removed` bytes, if the size is known.
    fn change(&self, added: u64, removed: u64) {
        let mut kept = self.lock();
        kept.changes += 1;
        if let Some(bytes) = kept.bytes.as_mut() {
            *bytes = (*bytes + added).saturating_sub(removed);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, KeptSize> {
        self.0
            .lock()
            .expect("cache size lock poisoned by an earlier panic")
    }
}

/// Names one author's blob in the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobKey(String);

impl BlobKey {
    pub fn new(author: &PublicKey, blob_id: &str) -> Result<Self> {
        ensure!(is_blob_id(blob_id), "malformed file id");
        Ok(Self(format!("{}-{blob_id}", member_id(author))))
    }
}

impl BlobCache {
    /// A cache that keeps everything.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            limit_bytes: None,
            size: CacheSize::default(),
        }
    }

    /// A cache trimmed to `limit_bytes`, whose size is kept in `size`.
    pub fn limited(root: PathBuf, limit_bytes: u64, size: CacheSize) -> Self {
        Self {
            root,
            limit_bytes: Some(limit_bytes),
            size,
        }
    }

    fn blob_dir(&self, blob: &BlobKey) -> PathBuf {
        self.root.join(&blob.0)
    }

    fn chunk_path(&self, blob: &BlobKey, index: u32) -> PathBuf {
        self.blob_dir(blob).join(index.to_string())
    }

    pub fn read_chunk(&self, blob: &BlobKey, index: u32) -> Result<Option<Vec<u8>>> {
        match fs::read(self.chunk_path(blob, index)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).context("reading a cached file piece"),
        }
    }

    /// Writes a piece whole or not at all, so a crash never leaves half a
    /// piece behind.
    pub fn write_chunk(&self, blob: &BlobKey, index: u32, sealed: &[u8]) -> Result<()> {
        fs::create_dir_all(self.blob_dir(blob)).context("making the file cache")?;
        let path = self.chunk_path(blob, index);
        let replaced = file_size(&path);
        let partial = path.with_extension("partial");
        fs::write(&partial, sealed).context("caching a file piece")?;
        fs::rename(&partial, &path).context("caching a file piece")?;
        self.size.change(sealed.len() as u64, replaced);
        Ok(())
    }

    pub fn forget_chunk(&self, blob: &BlobKey, index: u32) -> Result<()> {
        let path = self.chunk_path(blob, index);
        let removed = file_size(&path);
        match fs::remove_file(path) {
            Ok(()) => {
                self.size.change(0, removed);
                Ok(())
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("removing a cached file piece"),
        }
    }

    /// Marks a blob as just used, so trimming keeps it longest.
    pub fn touch(&self, blob: &BlobKey) -> Result<()> {
        let dir = self.blob_dir(blob);
        if !dir.exists() {
            return Ok(());
        }
        fs::File::create(dir.join(USED_MARKER))
            .and_then(|marker| marker.set_modified(SystemTime::now()))
            .context("marking a cached file as used")
    }

    pub fn forget(&self, blob: &BlobKey) -> Result<()> {
        let dir = self.blob_dir(blob);
        let removed = dir_size(&dir);
        match fs::remove_dir_all(dir) {
            Ok(()) => {
                match removed {
                    Ok(removed) => self.size.change(0, removed),
                    // Couldn't count it: measure again when next needed.
                    Err(_) => self.size.set(None),
                }
                Ok(())
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("removing a cached file"),
        }
    }

    /// Drops the blobs used longest ago until the cache fits its limit,
    /// never `keeping`.
    pub fn trim(&self, keeping: &BlobKey) -> Result<()> {
        let Some(limit) = self.limit_bytes else {
            return Ok(());
        };
        if self.size.known().is_some_and(|bytes| bytes <= limit) {
            return Ok(());
        }
        let changes_before = self.size.changes();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).context("reading the file cache"),
        };
        let mut blobs = Vec::new();
        for entry in entries {
            let entry = entry.context("reading the file cache")?;
            if !entry.file_type().context("reading the file cache")?.is_dir() {
                continue;
            }
            let path = entry.path();
            let used = fs::metadata(path.join(USED_MARKER))
                .or_else(|_| entry.metadata())
                .and_then(|metadata| metadata.modified())
                .context("reading the file cache")?;
            blobs.push((used, path.clone(), dir_size(&path)?));
        }
        let mut total: u64 = blobs.iter().map(|(_, _, size)| size).sum();
        blobs.sort();
        let kept = self.blob_dir(keeping);
        for (_, path, size) in blobs {
            if total <= limit {
                break;
            }
            if path != kept {
                if let Err(error) = fs::remove_dir_all(&path) {
                    self.size.counted(total, changes_before);
                    return Err(error).context("trimming the file cache");
                }
                total -= size;
            }
        }
        self.size.counted(total, changes_before);
        Ok(())
    }
}

/// A file's size, or 0 if it isn't there (or can't be read, which only
/// makes the kept size a little off until the next full count).
fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map_or(0, |metadata| metadata.len())
}

fn dir_size(dir: &Path) -> Result<u64> {
    let mut size = 0;
    for entry in fs::read_dir(dir).context("reading the file cache")? {
        size += entry
            .context("reading the file cache")?
            .metadata()
            .context("reading the file cache")?
            .len();
    }
    Ok(size)
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;

    const FIRST: &str = "00000000000000000000000000000001";
    const SECOND: &str = "00000000000000000000000000000002";

    fn key(blob_id: &str) -> BlobKey {
        BlobKey::new(&SecretKey::from_bytes(&[7; 32]).public(), blob_id).unwrap()
    }

    #[test]
    fn a_cached_piece_reads_back_until_forgotten() {
        let folder = tempfile::tempdir().unwrap();
        let cache = BlobCache::new(folder.path().to_owned());
        let blob = key(FIRST);
        assert_eq!(cache.read_chunk(&blob, 0).unwrap(), None);
        cache.write_chunk(&blob, 0, b"sealed").unwrap();
        assert_eq!(
            cache.read_chunk(&blob, 0).unwrap().as_deref(),
            Some(&b"sealed"[..])
        );
        cache.forget(&blob).unwrap();
        assert_eq!(cache.read_chunk(&blob, 0).unwrap(), None);
    }

    #[test]
    fn one_authors_blob_never_stands_in_for_anothers_with_the_same_id() {
        let folder = tempfile::tempdir().unwrap();
        let cache = BlobCache::new(folder.path().to_owned());
        let ada = BlobKey::new(&SecretKey::from_bytes(&[1; 32]).public(), FIRST).unwrap();
        let bob = BlobKey::new(&SecretKey::from_bytes(&[2; 32]).public(), FIRST).unwrap();
        cache.write_chunk(&bob, 0, b"junk").unwrap();
        assert_eq!(cache.read_chunk(&ada, 0).unwrap(), None);
    }

    #[test]
    fn trimming_drops_the_blob_used_longest_ago() {
        let folder = tempfile::tempdir().unwrap();
        let cache = BlobCache::limited(folder.path().to_owned(), 15, CacheSize::default());
        let (first, second) = (key(FIRST), key(SECOND));
        cache.write_chunk(&first, 0, &[0; 10]).unwrap();
        cache.touch(&first).unwrap();
        cache.write_chunk(&second, 0, &[0; 10]).unwrap();
        cache.touch(&second).unwrap();
        let old = SystemTime::now() - std::time::Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(cache.blob_dir(&first).join(USED_MARKER))
            .unwrap()
            .set_modified(old)
            .unwrap();

        cache.trim(&second).unwrap();
        assert_eq!(cache.read_chunk(&first, 0).unwrap(), None);
        assert!(cache.read_chunk(&second, 0).unwrap().is_some());
    }

    /// Measured once, the size is then kept up to date, so trimming a cache
    /// under its limit reads no folders, however many there are.
    #[cfg(unix)]
    #[test]
    fn trimming_under_the_limit_reads_no_folders() {
        use std::os::unix::fs::PermissionsExt;

        let folder = tempfile::tempdir().unwrap();
        let cache = BlobCache::limited(folder.path().to_owned(), 1_000, CacheSize::default());
        cache.write_chunk(&key(FIRST), 0, &[0; 10]).unwrap();
        cache.trim(&key(FIRST)).unwrap();
        let unreadable = folder.path().join("unreadable");
        fs::create_dir(&unreadable).unwrap();
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();

        cache.write_chunk(&key(SECOND), 0, &[0; 10]).unwrap();
        let trimmed = cache.trim(&key(SECOND));

        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o755)).unwrap();
        trimmed.unwrap();
    }

    /// Each use makes a new cache value, sharing one kept size. Writes and
    /// removals keep it right: if a removal weren't counted, the cache would
    /// look over its limit, and trimming would read the unreadable folder.
    #[cfg(unix)]
    #[test]
    fn the_kept_size_follows_writes_and_removals() {
        use std::os::unix::fs::PermissionsExt;

        let folder = tempfile::tempdir().unwrap();
        let size = CacheSize::default();
        let cache = || BlobCache::limited(folder.path().to_owned(), 25, size.clone());
        let write = |blob_id: &str| {
            cache().write_chunk(&key(blob_id), 0, &[0; 10]).unwrap();
            cache().trim(&key(blob_id))
        };
        write(FIRST).unwrap();
        write(SECOND).unwrap();
        let unreadable = folder.path().join("unreadable");
        fs::create_dir(&unreadable).unwrap();
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();

        cache().forget(&key(FIRST)).unwrap();
        let within_limit = write("00000000000000000000000000000003");
        let past_limit = write("00000000000000000000000000000004");

        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o755)).unwrap();
        within_limit.unwrap();
        assert!(past_limit.is_err(), "30 bytes are past the limit, so it counted");
    }

    #[test]
    fn ids_that_are_not_blob_ids_never_name_a_folder() {
        let author = SecretKey::from_bytes(&[7; 32]).public();
        assert!(BlobKey::new(&author, "../escape").is_err());
    }
}
