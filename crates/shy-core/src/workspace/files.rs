//! Sharing files and profile pictures. A file is encrypted into the local
//! cache, uploaded piece by piece, and only then announced in an op, so a
//! file op always means its pieces are in the bucket. Opening a file fetches
//! the pieces it lacks and checks each against the hashes its author signed.

use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use iroh::PublicKey;
use tracing::warn;

use super::{Workspace, actions::checked_message_text};
use crate::{
    files::{
        MAX_FILE_BYTES, MAX_PICTURE_BYTES, SharedFile,
        blob::{self, BlobRef},
        cache::BlobCache,
        clean_file_name, clean_media_type,
    },
    ops::{EntityId, MAX_MESSAGE_CHARS, Op},
    storage::{ItemKind, StoredFile},
    views::{Change, FileInfo, member_id},
};

pub const MAX_FILES_PER_MESSAGE: usize = 10;
const FILE_CACHE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const UPLOAD_ATTEMPTS: u32 = 3;

/// A file to share: its name, its media type as the sender's system knows
/// it, and where its bytes are.
pub struct NewFile {
    pub name: String,
    pub media_type: String,
    pub source: FileSource,
}

pub enum FileSource {
    Path(PathBuf),
    Bytes(Vec<u8>),
}

impl FileSource {
    fn size(&self) -> Result<u64> {
        Ok(match self {
            FileSource::Path(path) => std::fs::metadata(path)
                .with_context(|| format!("reading {}", path.display()))?
                .len(),
            FileSource::Bytes(bytes) => u64::try_from(bytes.len())?,
        })
    }
}

impl Workspace {
    fn file_cache(&self) -> BlobCache {
        BlobCache::new(self.inner.data.path.join("file-cache"), Some(FILE_CACHE_LIMIT))
    }

    fn picture_cache(&self) -> BlobCache {
        BlobCache::new(self.inner.data.path.join("picture-cache"), None)
    }

    /// Uploads each file, then sends one message carrying them, with `text`
    /// as its caption (it may be empty).
    pub async fn send_files(
        &self,
        channel: &str,
        text: &str,
        reply_to: Option<EntityId>,
        files: Vec<NewFile>,
    ) -> Result<EntityId> {
        ensure!(!files.is_empty(), "no files to send");
        ensure!(
            files.len() <= MAX_FILES_PER_MESSAGE,
            "a message carries at most {MAX_FILES_PER_MESSAGE} files"
        );
        let caption = if text.trim().is_empty() {
            ""
        } else {
            checked_message_text(text)?
        };
        ensure!(caption.chars().count() <= MAX_MESSAGE_CHARS, "message too long");
        self.require_item(channel, ItemKind::Channel)?;
        let mut sizes = Vec::new();
        for file in &files {
            let size = file.source.size()?;
            ensure!(
                size <= MAX_FILE_BYTES,
                "{} is larger than {} MB",
                file.name,
                MAX_FILE_BYTES / 1024 / 1024
            );
            sizes.push(size);
        }
        let total: u64 = sizes.iter().sum();
        let message = self.new_item_id(channel);
        let mut shared = Vec::new();
        let mut sent = 0;
        for (file, size) in files.into_iter().zip(sizes) {
            let uploaded = self
                .upload(self.file_cache(), file.source, size, |done| {
                    self.emit(Change::Upload {
                        channel: channel.to_owned(),
                        sent: sent + done,
                        total,
                    });
                })
                .await;
            let blob = match uploaded {
                Ok(blob) => blob,
                Err(error) => {
                    self.abandon_uploads(&shared).await;
                    return Err(error);
                }
            };
            sent += size;
            shared.push(SharedFile {
                id: self.new_item_id(channel),
                place: channel.to_owned(),
                message: Some(message.clone()),
                name: clean_file_name(&file.name),
                media_type: clean_media_type(&file.media_type),
                blob,
            });
        }
        for file in shared {
            self.write(Op::File { file })?;
        }
        self.post_message(channel, &message, caption, reply_to)?;
        Ok(message)
    }

    /// Encrypts `source` into `cache`, then uploads its pieces under our own
    /// prefix, reporting the plaintext bytes sent so far.
    async fn upload(
        &self,
        cache: BlobCache,
        source: FileSource,
        size: u64,
        progress: impl Fn(u64),
    ) -> Result<BlobRef> {
        let (cache, blob) = tokio::task::spawn_blocking(move || -> Result<(BlobCache, BlobRef)> {
            let blob = match source {
                FileSource::Path(path) => {
                    let mut file =
                        std::fs::File::open(&path).with_context(|| format!("opening {}", path.display()))?;
                    seal_into(&cache, &mut file, size)?
                }
                FileSource::Bytes(bytes) => seal_into(&cache, &mut bytes.as_slice(), size)?,
            };
            Ok((cache, blob))
        })
        .await
        .context("encrypting the file")??;
        let bucket = self.bucket();
        let me = self.me();
        for index in 0..blob.chunk_count() {
            let sealed = cache
                .read_chunk(&blob.id, index)?
                .context("a file piece vanished from the cache")?;
            let mut attempt = 1;
            loop {
                match bucket.put_file_chunk(&me, &blob.id, index, sealed.clone()).await {
                    Ok(()) => break,
                    Err(error) if attempt < UPLOAD_ATTEMPTS => {
                        warn!("retrying a file upload: {error:#}");
                        tokio::time::sleep(Duration::from_secs(u64::from(attempt))).await;
                        attempt += 1;
                    }
                    Err(error) => {
                        self.abandon_blob(&blob.id).await;
                        return Err(error.context("uploading the file"));
                    }
                }
            }
            progress(
                (u64::from(index) + 1)
                    .saturating_mul(u64::from(blob.chunk_size))
                    .min(size),
            );
        }
        Ok(blob)
    }

    async fn abandon_uploads(&self, shared: &[SharedFile]) {
        for file in shared {
            self.abandon_blob(&file.blob.id).await;
        }
    }

    /// Takes a blob nobody will hear about out of the bucket, as far as we
    /// can; what stays behind is only unreadable bytes.
    async fn abandon_blob(&self, blob_id: &str) {
        if let Err(error) = self.bucket().delete_blob(&self.me(), blob_id).await {
            warn!("couldn't remove an abandoned upload: {error:#}");
        }
    }

    /// A file's whole contents, checked. For showing images and small files;
    /// use [`Workspace::save_file`] for anything large.
    pub async fn file_bytes(&self, file: &str) -> Result<Vec<u8>> {
        let stored = self.stored_file(file)?;
        let mut content = Vec::with_capacity(usize::try_from(stored.blob.size)?);
        self.read_blob(&self.file_cache(), &stored, &mut content).await?;
        Ok(content)
    }

    /// Writes a file's contents to `destination`, replacing it only once
    /// every piece has arrived and checked out.
    pub async fn save_file(&self, file: &str, destination: &Path) -> Result<()> {
        let stored = self.stored_file(file)?;
        let partial = destination.with_extension("shyspace-partial");
        let mut output =
            std::fs::File::create(&partial).with_context(|| format!("writing {}", partial.display()))?;
        let result = self.read_blob(&self.file_cache(), &stored, &mut output).await;
        if let Err(error) = result {
            drop(output);
            if let Err(cleanup) = std::fs::remove_file(&partial) {
                warn!("couldn't remove a partly saved file: {cleanup}");
            }
            return Err(error);
        }
        output.sync_all().context("saving the file")?;
        std::fs::rename(&partial, destination)
            .with_context(|| format!("saving {}", destination.display()))?;
        Ok(())
    }

    fn stored_file(&self, file: &str) -> Result<StoredFile> {
        self.state().store.file(file)?.context("no such file here")
    }

    async fn read_blob(&self, cache: &BlobCache, stored: &StoredFile, output: &mut impl Write) -> Result<()> {
        let author = PublicKey::try_from(stored.author.as_slice()).context("reading a file's author")?;
        read_blob_into(&self.bucket(), cache, &author, &stored.blob, output).await
    }

    /// Deletes a file: it leaves everyone's app, and its pieces leave the
    /// bucket. Its author, the admin, or a private channel's owner may.
    pub fn delete_file(&self, file: &str) -> Result<()> {
        self.require_item(file, ItemKind::File)?;
        self.write(Op::Delete { id: file.to_owned() })?;
        self.inner.wake.notify_one();
        Ok(())
    }

    /// Every file this member can see, newest first.
    pub fn files(&self) -> Result<Vec<FileInfo>> {
        let state = self.state();
        let stored = state.store.all_files()?;
        stored
            .into_iter()
            .map(|file| self.file_info(&state.store, file))
            .collect()
    }

    pub(crate) fn file_info(&self, store: &crate::storage::Store, file: StoredFile) -> Result<FileInfo> {
        Ok(FileInfo {
            can_delete: self.may_delete(store, &file.id)?,
            author: PublicKey::try_from(file.author.as_slice())
                .map(|author| member_id(&author))
                .context("reading a file's author")?,
            size: file.blob.size,
            id: file.id,
            place: file.place,
            message: file.message,
            name: file.name,
            media_type: file.media_type,
            created_ms: file.created_ms,
        })
    }

    /// Shares a new picture (an already cropped PNG, JPEG or WebP), or
    /// removes ours with `None`. The old one leaves the bucket.
    pub async fn set_profile_picture(&self, image: Option<Vec<u8>>) -> Result<()> {
        let picture = match image {
            Some(image) => {
                let size = u64::try_from(image.len())?;
                ensure!(size <= MAX_PICTURE_BYTES, "the picture is too large");
                ensure!(is_picture(&image), "a picture must be a PNG, JPEG or WebP image");
                Some(
                    self.upload(self.picture_cache(), FileSource::Bytes(image), size, |_| {})
                        .await?,
                )
            }
            None => None,
        };
        self.write(Op::ProfilePicture { picture })?;
        self.inner.wake.notify_one();
        Ok(())
    }

    /// A member's picture, if they shared one.
    pub async fn profile_picture(&self, member: &PublicKey) -> Result<Option<Vec<u8>>> {
        let Some(picture) = self.state().store.profile_picture(member.as_bytes())? else {
            return Ok(None);
        };
        let mut content = Vec::new();
        read_blob_into(
            &self.bucket(),
            &self.picture_cache(),
            member,
            &picture,
            &mut content,
        )
        .await?;
        Ok(Some(content))
    }

    /// Takes deleted files and replaced pictures out of the bucket: ours, or
    /// ones we deleted. Every device drops its own cached copy.
    pub(crate) async fn remove_unused_blobs(&self) -> Result<()> {
        let removals = self.state().store.blob_removals()?;
        let me = self.me();
        for removal in removals {
            self.file_cache().forget(&removal.blob_id)?;
            self.picture_cache().forget(&removal.blob_id)?;
            let is_ours = removal.author == me.as_bytes() || removal.requested_by == me.as_bytes();
            if is_ours {
                let author = PublicKey::try_from(removal.author.as_slice())
                    .context("reading a removed file's author")?;
                self.bucket().delete_blob(&author, &removal.blob_id).await?;
            }
            self.state().store.forget_blob_removal(&removal)?;
        }
        Ok(())
    }
}

fn seal_into(cache: &BlobCache, source: &mut impl std::io::Read, size: u64) -> Result<BlobRef> {
    let mut started: Option<String> = None;
    let sealed = blob::seal(source, size, |blob_id, index, piece| {
        started.get_or_insert_with(|| blob_id.to_owned());
        cache.write_chunk(blob_id, index, piece)
    });
    if sealed.is_err()
        && let Some(blob_id) = started
        && let Err(error) = cache.forget(&blob_id)
    {
        warn!("couldn't remove a half encrypted file: {error:#}");
    }
    sealed
}

async fn read_blob_into(
    bucket: &crate::transport::bucket::Bucket,
    cache: &BlobCache,
    author: &PublicKey,
    blob: &BlobRef,
    output: &mut impl Write,
) -> Result<()> {
    for index in 0..blob.chunk_count() {
        let sealed = match cache.read_chunk(&blob.id, index)? {
            Some(sealed) => sealed,
            None => {
                let fetched = bucket
                    .file_chunk(author, &blob.id, index, blob::sealed_chunk_len(blob, index))
                    .await?
                    .context("a piece of this file is missing from the bucket")?;
                let expected = blob.chunk_hashes.get(usize::try_from(index)?);
                ensure!(
                    expected == Some(blake3::hash(&fetched).as_bytes()),
                    "a piece of this file doesn't match what its author signed"
                );
                cache.write_chunk(&blob.id, index, &fetched)?;
                fetched.to_vec()
            }
        };
        output
            .write_all(&blob.open_chunk(index, &sealed)?)
            .context("writing the file")?;
    }
    cache.touch(&blob.id)?;
    cache.trim(&blob.id)?;
    Ok(())
}

/// PNG, JPEG or WebP, by their first bytes.
fn is_picture(image: &[u8]) -> bool {
    image.starts_with(b"\x89PNG\r\n\x1a\n")
        || image.starts_with(&[0xff, 0xd8, 0xff])
        || (image.len() >= 12 && &image[..4] == b"RIFF" && &image[8..12] == b"WEBP")
}
