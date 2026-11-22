//! Sealed file pieces kept on this device, so a file opened once opens again
//! without the network. Pieces stay encrypted here: their keys live only in
//! the encrypted database, so a copied data folder still reveals nothing.

use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, Result, ensure};

use super::blob::is_blob_id;

pub struct BlobCache {
    root: PathBuf,
    /// Beyond this many bytes, the blobs opened longest ago are dropped.
    /// `None` keeps everything (profile pictures).
    limit_bytes: Option<u64>,
}

impl BlobCache {
    pub fn new(root: PathBuf, limit_bytes: Option<u64>) -> Self {
        Self { root, limit_bytes }
    }

    fn blob_dir(&self, blob_id: &str) -> Result<PathBuf> {
        ensure!(is_blob_id(blob_id), "malformed file id");
        Ok(self.root.join(blob_id))
    }

    fn chunk_path(&self, blob_id: &str, index: u32) -> Result<PathBuf> {
        Ok(self.blob_dir(blob_id)?.join(index.to_string()))
    }

    pub fn read_chunk(&self, blob_id: &str, index: u32) -> Result<Option<Vec<u8>>> {
        match fs::read(self.chunk_path(blob_id, index)?) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).context("reading a cached file piece"),
        }
    }

    /// Writes a piece whole or not at all, so a crash never leaves half a
    /// piece behind.
    pub fn write_chunk(&self, blob_id: &str, index: u32, sealed: &[u8]) -> Result<()> {
        let dir = self.blob_dir(blob_id)?;
        fs::create_dir_all(&dir).context("making the file cache")?;
        let path = self.chunk_path(blob_id, index)?;
        let partial = path.with_extension("partial");
        fs::write(&partial, sealed).context("caching a file piece")?;
        fs::rename(&partial, &path).context("caching a file piece")?;
        Ok(())
    }

    /// Marks a blob as just used, so trimming keeps it longest.
    pub fn touch(&self, blob_id: &str) -> Result<()> {
        let dir = self.blob_dir(blob_id)?;
        match fs::File::open(&dir) {
            Ok(handle) => handle
                .set_modified(SystemTime::now())
                .context("marking a cached file as used"),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("marking a cached file as used"),
        }
    }

    pub fn forget(&self, blob_id: &str) -> Result<()> {
        match fs::remove_dir_all(self.blob_dir(blob_id)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("removing a cached file"),
        }
    }

    /// Drops the blobs used longest ago until the cache fits its limit,
    /// never `keeping`.
    pub fn trim(&self, keeping: &str) -> Result<()> {
        let Some(limit) = self.limit_bytes else {
            return Ok(());
        };
        let mut blobs = Vec::new();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).context("reading the file cache"),
        };
        for entry in entries {
            let entry = entry.context("reading the file cache")?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_blob_id(&name) {
                continue;
            }
            let used = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .context("reading the file cache")?;
            blobs.push((used, name.clone(), dir_size(&entry.path())?));
        }
        let mut total: u64 = blobs.iter().map(|(_, _, size)| size).sum();
        blobs.sort();
        for (_, name, size) in blobs {
            if total <= limit {
                break;
            }
            if name != keeping {
                self.forget(&name)?;
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
    use super::*;

    const FIRST: &str = "00000000000000000000000000000001";
    const SECOND: &str = "00000000000000000000000000000002";

    #[test]
    fn a_cached_piece_reads_back_until_forgotten() {
        let folder = tempfile::tempdir().unwrap();
        let cache = BlobCache::new(folder.path().to_owned(), None);
        assert_eq!(cache.read_chunk(FIRST, 0).unwrap(), None);
        cache.write_chunk(FIRST, 0, b"sealed").unwrap();
        assert_eq!(
            cache.read_chunk(FIRST, 0).unwrap().as_deref(),
            Some(&b"sealed"[..])
        );
        cache.forget(FIRST).unwrap();
        assert_eq!(cache.read_chunk(FIRST, 0).unwrap(), None);
    }

    #[test]
    fn trimming_drops_the_blob_used_longest_ago() {
        let folder = tempfile::tempdir().unwrap();
        let cache = BlobCache::new(folder.path().to_owned(), Some(15));
        cache.write_chunk(FIRST, 0, &[0; 10]).unwrap();
        cache.write_chunk(SECOND, 0, &[0; 10]).unwrap();
        let old = SystemTime::now() - std::time::Duration::from_secs(60);
        fs::File::open(folder.path().join(FIRST))
            .unwrap()
            .set_modified(old)
            .unwrap();

        cache.trim(SECOND).unwrap();
        assert_eq!(cache.read_chunk(FIRST, 0).unwrap(), None);
        assert!(cache.read_chunk(SECOND, 0).unwrap().is_some());
    }

    #[test]
    fn ids_that_are_not_blob_ids_never_reach_the_disk() {
        let folder = tempfile::tempdir().unwrap();
        let cache = BlobCache::new(folder.path().to_owned(), None);
        assert!(cache.write_chunk("../escape", 0, b"x").is_err());
        assert!(cache.read_chunk("../escape", 0).is_err());
    }
}
