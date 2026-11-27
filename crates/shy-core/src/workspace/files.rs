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
        cache::{BlobCache, BlobKey},
        clean_file_name, clean_media_type,
    },
    ops::{EntityId, MAX_MESSAGE_CHARS, Op, new_owned_id},
    storage::{BlobRemoval, ItemKind, StoredFile},
    views::{Change, FileInfo, member_id},
};

pub const MAX_FILES_PER_MESSAGE: usize = 10;
const FILE_CACHE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const UPLOAD_ATTEMPTS: u32 = 3;
/// Files opened whole in memory (to show them) stay at most this large.
const MAX_OPENED_IN_MEMORY: u64 = 64 * 1024 * 1024;

/// A file to share: its name, its media type as the sender's system knows
/// it, and where its bytes are.
pub struct NewFile {
    pub name: String,
    pub media_type: String,
    pub source: FileSource,
}

impl NewFile {
    fn checked_size(&self) -> Result<u64> {
        let size = self.source.size()?;
        ensure!(
            size <= MAX_FILE_BYTES,
            "{} is larger than {} MB",
            self.name,
            MAX_FILE_BYTES / 1024 / 1024
        );
        Ok(size)
    }
}

#[derive(Clone)]
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
        BlobCache::limited(
            self.inner.data.path.join("file-cache"),
            FILE_CACHE_LIMIT,
            self.inner.file_cache_size.clone(),
        )
    }

    fn picture_cache(&self) -> BlobCache {
        BlobCache::new(self.inner.data.path.join("picture-cache"))
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
        if let Some(target) = &reply_to {
            self.ensure_can_reply_to(channel, target)?;
        }
        let sizes = files
            .iter()
            .map(NewFile::checked_size)
            .collect::<Result<Vec<_>>>()?;
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
        let blob_ids: Vec<String> = shared.iter().map(|file| file.blob.id.clone()).collect();
        let message_op = Op::Message {
            id: message.clone(),
            channel: channel.to_owned(),
            text: caption.to_owned(),
            reply_to,
        };
        // The files and their message go out together, or not at all.
        let written = shared
            .into_iter()
            .map(|file| Op::File { file })
            .chain([message_op])
            .map(|op| self.content_maker(op))
            .collect::<Result<Vec<_>>>()
            .and_then(|makers| self.write_together(makers));
        if let Err(error) = written {
            let cache = self.file_cache();
            for blob_id in &blob_ids {
                self.abandon_blob(&cache, blob_id).await;
            }
            return Err(error);
        }
        self.after_posting(channel, &message)?;
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
        ensure!(!self.is_reading_only(), super::reading::READING_ONLY);
        let me = self.me();
        let (cache, blob) = tokio::task::spawn_blocking(move || -> Result<(BlobCache, BlobRef)> {
            let blob = match source {
                FileSource::Path(path) => {
                    let mut file =
                        std::fs::File::open(&path).with_context(|| format!("opening {}", path.display()))?;
                    seal_into(&cache, &me, &mut file, size)?
                }
                FileSource::Bytes(bytes) => seal_into(&cache, &me, &mut bytes.as_slice(), size)?,
            };
            Ok((cache, blob))
        })
        .await
        .context("encrypting the file")??;
        if let Err(error) = self.put_blob(&cache, &blob, size, progress).await {
            self.abandon_blob(&cache, &blob.id).await;
            return Err(error);
        }
        // The upload worked; a cache that can't shrink right now is only worth a warning.
        let trimmed = BlobKey::new(&me, &blob.id).and_then(|key| cache.trim(&key));
        if let Err(error) = trimmed {
            warn!("couldn't trim the file cache: {error:#}");
        }
        Ok(blob)
    }

    /// Uploads a sealed blob's pieces from the cache, retrying each a few
    /// times.
    async fn put_blob(
        &self,
        cache: &BlobCache,
        blob: &BlobRef,
        size: u64,
        progress: impl Fn(u64),
    ) -> Result<()> {
        let bucket = self.bucket();
        let me = self.me();
        let cached = BlobKey::new(&me, &blob.id)?;
        for index in 0..blob.chunk_count() {
            let sealed = cache
                .read_chunk(&cached, index)?
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
                    Err(error) => return Err(error.context("uploading the file")),
                }
            }
            progress(
                (u64::from(index) + 1)
                    .saturating_mul(u64::from(blob.chunk_size))
                    .min(size),
            );
        }
        Ok(())
    }

    async fn abandon_uploads(&self, shared: &[SharedFile]) {
        let cache = self.file_cache();
        for file in shared {
            self.abandon_blob(&cache, &file.blob.id).await;
        }
    }

    /// Takes a blob nobody will hear about out of the bucket and the cache,
    /// as far as we can; what stays behind is only unreadable bytes.
    async fn abandon_blob(&self, cache: &BlobCache, blob_id: &str) {
        if let Err(error) = self.bucket().delete_blob(&self.me(), blob_id).await {
            warn!("couldn't remove an abandoned upload: {error:#}");
        }
        let forgotten = BlobKey::new(&self.me(), blob_id).and_then(|key| cache.forget(&key));
        if let Err(error) = forgotten {
            warn!("couldn't remove an abandoned upload from the cache: {error:#}");
        }
    }

    /// A file's whole contents, checked, for showing in place. Larger files
    /// are refused; [`Workspace::save_file`] streams them to disk instead.
    pub async fn file_bytes(&self, file: &str) -> Result<Vec<u8>> {
        let stored = self.stored_file(file)?;
        ensure!(
            stored.blob.size <= MAX_OPENED_IN_MEMORY,
            "this file is too large to open here; save it instead"
        );
        let mut content = Vec::new();
        self.read_blob(&self.file_cache(), &stored, &mut content).await?;
        Ok(content)
    }

    /// Writes a file's contents to `destination`, replacing it only once
    /// every piece has arrived and checked out.
    pub async fn save_file(&self, file: &str, destination: &Path) -> Result<()> {
        let stored = self.stored_file(file)?;
        let mut partial_name = destination.as_os_str().to_owned();
        partial_name.push(".shyspace-partial");
        let partial = PathBuf::from(partial_name);
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

    /// Uploads `file` into `page`, for everyone who can read the page.
    /// Returns its id, for the page's text to link.
    pub async fn share_page_file(&self, page: &str, file: NewFile) -> Result<EntityId> {
        self.require_item(page, ItemKind::Page)?;
        let size = file.checked_size()?;
        let blob = self
            .upload(self.file_cache(), file.source, size, |done| {
                self.emit(Change::Upload {
                    channel: page.to_owned(),
                    sent: done,
                    total: size,
                });
            })
            .await?;
        let blob_id = blob.id.clone();
        let shared = SharedFile {
            id: new_owned_id(&self.me()),
            place: page.to_owned(),
            message: None,
            name: clean_file_name(&file.name),
            media_type: clean_media_type(&file.media_type),
            blob,
        };
        let id = shared.id.clone();
        if let Err(error) = self.write(Op::File { file: shared }) {
            self.abandon_blob(&self.file_cache(), &blob_id).await;
            return Err(error);
        }
        self.inner.wake.notify_one();
        Ok(id)
    }

    /// The files shared into `page`, oldest first.
    pub fn page_files(&self, page: &str) -> Result<Vec<FileInfo>> {
        let state = self.state();
        let epoch = state.store.newest_epoch()?;
        state
            .store
            .files_of_page(page)?
            .into_iter()
            .map(|file| self.file_info(&state.store, epoch, file))
            .collect()
    }

    /// A file of `page`'s, whole, for showing in the page.
    pub async fn page_file_bytes(&self, page: &str, file: &str) -> Result<Vec<u8>> {
        self.require_page_file(page, file)?;
        self.file_bytes(file).await
    }

    /// Saves a file of `page`'s to `destination`, like [`Workspace::save_file`].
    pub async fn save_page_file(&self, page: &str, file: &str, destination: &Path) -> Result<()> {
        self.require_page_file(page, file)?;
        self.save_file(file, destination).await
    }

    /// A file shared anywhere but `page` is refused, so a page's text can't
    /// pull one in by naming its id.
    fn require_page_file(&self, page: &str, file: &str) -> Result<()> {
        let stored = self.stored_file(file)?;
        ensure!(
            stored.place == page && stored.message.is_none(),
            "that file isn't part of this page"
        );
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

    /// Gives a file you shared a new name. Its bytes stay as they are.
    pub fn rename_file(&self, file: &str, name: &str) -> Result<()> {
        self.require_item(file, ItemKind::File)?;
        let name = name.trim();
        ensure!(!name.is_empty(), "a file needs a name");
        let cleaned = clean_file_name(name);
        ensure!(
            cleaned == name,
            "a file name can't hold slashes, hidden characters or a leading dot"
        );
        let rename = Op::FileRename {
            id: file.to_owned(),
            name: cleaned,
        };
        let may_rename = {
            let store = &self.state().store;
            self.may(store, store.newest_epoch()?, &rename)?
        };
        ensure!(may_rename, "only the person who shared a file can rename it");
        self.write(rename)
    }

    /// Every file this member can see, newest first.
    pub fn files(&self) -> Result<Vec<FileInfo>> {
        let state = self.state();
        let stored = state.store.all_files()?;
        let epoch = state.store.newest_epoch()?;
        stored
            .into_iter()
            .map(|file| self.file_info(&state.store, epoch, file))
            .collect()
    }

    pub(crate) fn file_info(
        &self,
        store: &crate::storage::Store,
        epoch: u64,
        file: StoredFile,
    ) -> Result<FileInfo> {
        Ok(FileInfo {
            can_delete: self.may_delete(store, epoch, &file.id)?,
            can_rename: self.may(
                store,
                epoch,
                &Op::FileRename {
                    id: file.id.clone(),
                    name: String::new(),
                },
            )?,
            author: member_id(&store.person_of(
                &PublicKey::try_from(file.author.as_slice()).context("reading a file's author")?,
            )?),
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
        self.share_picture(image, |picture| Op::ProfilePicture { picture })
            .await
    }

    /// Uploads `image` (checked like every picture), or nothing for `None`,
    /// and writes the op `to_op` makes of it. The upload leaves the bucket
    /// again if the op can't be written.
    pub(super) async fn share_picture(
        &self,
        image: Option<Vec<u8>>,
        to_op: impl FnOnce(Option<BlobRef>) -> Op,
    ) -> Result<()> {
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
        let blob_id = picture.as_ref().map(|picture| picture.id.clone());
        if let Err(error) = self.write(to_op(picture)) {
            if let Some(blob_id) = blob_id {
                self.abandon_blob(&self.picture_cache(), &blob_id).await;
            }
            return Err(error);
        }
        self.inner.wake.notify_one();
        Ok(())
    }

    /// A person's picture: the newest any of their devices set.
    pub async fn profile_picture(&self, person: &PublicKey) -> Result<Option<Vec<u8>>> {
        let Some((device, picture)) = self.state().store.person_picture(person)? else {
            return Ok(None);
        };
        self.picture_bytes(&device, Some(picture)).await
    }

    /// The bytes of `owner`'s `picture`, from the cache or the bucket.
    pub(super) async fn picture_bytes(
        &self,
        owner: &PublicKey,
        picture: Option<BlobRef>,
    ) -> Result<Option<Vec<u8>>> {
        let Some(picture) = picture else {
            return Ok(None);
        };
        let mut content = Vec::new();
        read_blob_into(
            &self.bucket(),
            &self.picture_cache(),
            owner,
            &picture,
            &mut content,
        )
        .await?;
        Ok(Some(content))
    }

    /// Takes the blobs we deleted (or replaced) out of the bucket. Only the
    /// device that asked does, so a removal can't follow from a delete that
    /// turns out not to count elsewhere. Every device drops its own cached
    /// copy. One failure doesn't hold up the rest; the first is returned.
    pub(crate) async fn remove_unused_blobs(&self) -> Result<()> {
        let removals = self.state().store.blob_removals()?;
        let me = self.me();
        let mut first_error = None;
        for removal in removals {
            let removed = self.remove_blob(&removal, &me).await;
            match removed {
                Ok(()) => self.state().store.forget_blob_removal(&removal)?,
                Err(error) => {
                    warn!("couldn't remove a deleted file from the bucket: {error:#}");
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn remove_blob(&self, removal: &BlobRemoval, me: &PublicKey) -> Result<()> {
        let author =
            PublicKey::try_from(removal.author.as_slice()).context("reading a removed file's author")?;
        let cached = BlobKey::new(&author, &removal.blob_id)?;
        self.file_cache().forget(&cached)?;
        self.picture_cache().forget(&cached)?;
        if removal.requested_by == me.as_bytes() {
            self.bucket().delete_blob(&author, &removal.blob_id).await?;
        }
        Ok(())
    }
}

fn seal_into(
    cache: &BlobCache,
    me: &PublicKey,
    source: &mut impl std::io::Read,
    size: u64,
) -> Result<BlobRef> {
    let mut started: Option<BlobKey> = None;
    let sealed = blob::seal(source, size, |blob_id, index, piece| {
        let cached = match &started {
            Some(cached) => cached,
            None => started.insert(BlobKey::new(me, blob_id)?),
        };
        cache.write_chunk(cached, index, piece)?;
        if index == 0 {
            // So trimming, which goes by last use, never takes a blob
            // that is still being written.
            cache.touch(cached)?;
        }
        Ok(())
    });
    if sealed.is_err()
        && let Some(cached) = started
        && let Err(error) = cache.forget(&cached)
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
    let cached = BlobKey::new(author, &blob.id)?;
    for index in 0..blob.chunk_count() {
        let opened = match cache.read_chunk(&cached, index)? {
            Some(sealed) => match blob.open_chunk(index, &sealed) {
                Ok(opened) => opened,
                // A damaged cached piece is dropped and fetched again.
                Err(_) => {
                    cache.forget_chunk(&cached, index)?;
                    fetch_chunk(bucket, cache, author, blob, &cached, index).await?
                }
            },
            None => fetch_chunk(bucket, cache, author, blob, &cached, index).await?,
        };
        output.write_all(&opened).context("writing the file")?;
    }
    // Every piece was read and checked; a cache that can't be tidied right
    // now is only worth a warning, as after an upload.
    for tidied in [cache.touch(&cached), cache.trim(&cached)] {
        if let Err(error) = tidied {
            warn!("couldn't tidy the file cache: {error:#}");
        }
    }
    Ok(())
}

/// Fetches a piece, checks it against its signed hash, keeps it, and opens it.
async fn fetch_chunk(
    bucket: &crate::transport::bucket::Bucket,
    cache: &BlobCache,
    author: &PublicKey,
    blob: &BlobRef,
    cached: &BlobKey,
    index: u32,
) -> Result<Vec<u8>> {
    let fetched = bucket
        .file_chunk(author, &blob.id, index, blob::sealed_chunk_len(blob, index))
        .await?
        .context("a piece of this file is missing from the bucket")?;
    let opened = blob.open_chunk(index, &fetched)?;
    cache.write_chunk(cached, index, &fetched)?;
    Ok(opened)
}

/// PNG, JPEG or WebP, by their first bytes.
fn is_picture(image: &[u8]) -> bool {
    image.starts_with(b"\x89PNG\r\n\x1a\n")
        || image.starts_with(&[0xff, 0xd8, 0xff])
        || (image.len() >= 12 && &image[..4] == b"RIFF" && &image[8..12] == b"WEBP")
}
