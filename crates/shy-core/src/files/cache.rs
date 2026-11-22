//! Sealed file pieces kept on this device, so a file opened once opens again
//! without the network. Pieces stay encrypted here: their keys live only in
//! the encrypted database, so a copied data folder still reveals nothing.
//! Blobs are kept by author and id: ids are public, so anyone could reuse
//! one under their own prefix.

use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
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
    pub fn new(root: PathBuf, limit_bytes: Option<u64>) -> Self {
        Self { root, limit_bytes }
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
        let partial = path.with_extension("partial");
        fs::write(&partial, sealed).context("caching a file piece")?;
        fs::rename(&partial, &path).context("caching a file piece")?;
        Ok(())
    }

    pub fn forget_chunk(&self, blob: &BlobKey, index: u32) -> Result<()> {
        match fs::remove_file(self.chunk_path(blob, index)) {
            Ok(()) => Ok(()),
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
        match fs::remove_dir_all(self.blob_dir(blob)) {
            Ok(()) => Ok(()),
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
                fs::remove_dir_all(&path).context("trimming the file cache")?;
                total -= size;
            }
        }
        Ok(())
    }
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
        let cache = BlobCache::new(folder.path().to_owned(), None);
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
        let cache = BlobCache::new(folder.path().to_owned(), None);
        let ada = BlobKey::new(&SecretKey::from_bytes(&[1; 32]).public(), FIRST).unwrap();
        let bob = BlobKey::new(&SecretKey::from_bytes(&[2; 32]).public(), FIRST).unwrap();
        cache.write_chunk(&bob, 0, b"junk").unwrap();
        assert_eq!(cache.read_chunk(&ada, 0).unwrap(), None);
    }

    #[test]
    fn trimming_drops_the_blob_used_longest_ago() {
        let folder = tempfile::tempdir().unwrap();
        let cache = BlobCache::new(folder.path().to_owned(), Some(15));
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

    #[test]
    fn ids_that_are_not_blob_ids_never_name_a_folder() {
        let author = SecretKey::from_bytes(&[7; 32]).public();
        assert!(BlobKey::new(&author, "../escape").is_err());
    }
}
