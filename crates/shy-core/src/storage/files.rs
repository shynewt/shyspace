//! Shared files and profile pictures, and the bucket removals they leave
//! behind. A blob stays in the bucket until its file is deleted or its
//! picture replaced; then the author's device, or whoever deleted it, takes
//! it out (see `files::transfer`).

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, Row, named_params, params};

use super::Store;
use crate::{
    files::{SharedFile, blob::BlobRef, clean_file_name, clean_media_type},
    ops::VerifiedOp,
};

/// A file as every member sees it.
#[derive(Debug, Clone)]
pub struct StoredFile {
    pub id: String,
    pub place: String,
    pub message: Option<String>,
    pub author: Vec<u8>,
    pub name: String,
    pub media_type: String,
    pub created_ms: i64,
    pub blob: BlobRef,
}

/// A blob that no file or picture uses any more, waiting to leave the
/// bucket. Only the author's device or the one that asked acts on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRemoval {
    pub author: Vec<u8>,
    pub blob_id: String,
    pub requested_by: Vec<u8>,
}

const FILE_COLUMNS: &str = "id, place, message, author, name, media_type, created_ms, blob";

impl Store {
    /// Keeps a file from someone else, cleaned. The first op with an id wins.
    pub(super) fn put_file(&self, file: &SharedFile, verified: &VerifiedOp) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO files (id, place, message, author, name, media_type, size, blob, created_ms)
             VALUES (:id, :place, :message, :author, :name, :media_type, :size, :blob, :now)",
            named_params! {
                ":id": file.id,
                ":place": file.place,
                ":message": file.message,
                ":author": verified.author.as_bytes().as_slice(),
                ":name": clean_file_name(&file.name),
                ":media_type": clean_media_type(&file.media_type),
                ":size": i64::try_from(file.blob.size)?,
                ":blob": postcard::to_stdvec(&file.blob)?,
                ":now": verified.body.timestamp_ms,
            },
        )?;
        Ok(())
    }

    /// Removes the files `id` names: the file itself, a message's files or a
    /// channel's. Each is marked deleted, so it can't come back, and its blob
    /// waits to leave the bucket. Returns the places that lost files.
    pub(super) fn purge_files_of(&self, id: &str, deleter: &[u8]) -> Result<Vec<String>> {
        let doomed: Vec<(String, String, Vec<u8>, Vec<u8>)> = self
            .db
            .prepare("SELECT id, place, author, blob FROM files WHERE id = ?1 OR message = ?1 OR place = ?1")?
            .query_map([id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<_, _>>()?;
        let mut places = Vec::new();
        for (file, place, author, blob) in doomed {
            let blob: BlobRef = postcard::from_bytes(&blob).context("reading a stored file")?;
            self.mark_deleted(&file)?;
            self.db.execute("DELETE FROM files WHERE id = ?", [&file])?;
            self.request_blob_removal(&author, &blob.id, deleter)?;
            if !places.contains(&place) {
                places.push(place);
            }
        }
        Ok(places)
    }

    pub fn file(&self, id: &str) -> Result<Option<StoredFile>> {
        Ok(self
            .db
            .query_row(
                &format!("SELECT {FILE_COLUMNS} FROM files WHERE id = ?"),
                [id],
                stored_file,
            )
            .optional()?)
    }

    /// The files shared with these messages, oldest first.
    pub fn files_of_messages(&self, messages: &[String]) -> Result<Vec<StoredFile>> {
        let mut statement = self.db.prepare(&format!(
            "SELECT {FILE_COLUMNS} FROM files WHERE message = ? ORDER BY created_ms, id"
        ))?;
        let mut files = Vec::new();
        for message in messages {
            for file in statement.query_map([message], stored_file)? {
                files.push(file?);
            }
        }
        Ok(files)
    }

    /// Every file this device can see, newest first.
    pub fn all_files(&self) -> Result<Vec<StoredFile>> {
        self.db
            .prepare(&format!(
                "SELECT {FILE_COLUMNS} FROM files ORDER BY created_ms DESC, id"
            ))?
            .query_map([], stored_file)?
            .collect::<Result<_, _>>()
            .map_err(Into::into)
    }

    /// Keeps a member's picture unless a newer one is kept. Whichever loses
    /// leaves the bucket, through its author's device. Returns whether the
    /// picture shown changed.
    pub(super) fn put_profile_picture(
        &self,
        member: &[u8],
        picture: Option<&BlobRef>,
        lamport: u64,
    ) -> Result<bool> {
        let kept = self.kept_picture(member)?;
        let is_newer = kept
            .as_ref()
            .is_none_or(|(_, kept_lamport)| *kept_lamport < lamport);
        if !is_newer {
            if let Some(picture) = picture {
                self.request_blob_removal(member, &picture.id, member)?;
            }
            return Ok(false);
        }
        if let Some((Some(old), _)) = &kept {
            self.request_blob_removal(member, &old.id, member)?;
        }
        let encoded = picture.map(postcard::to_stdvec).transpose()?;
        self.db.execute(
            "INSERT INTO profile_pictures (member, blob, lamport) VALUES (?1, ?2, ?3)
             ON CONFLICT (member) DO UPDATE SET blob = ?2, lamport = ?3",
            params![member, encoded, lamport],
        )?;
        Ok(true)
    }

    pub fn profile_picture(&self, member: &[u8]) -> Result<Option<BlobRef>> {
        Ok(self.kept_picture(member)?.and_then(|(picture, _)| picture))
    }

    fn kept_picture(&self, member: &[u8]) -> Result<Option<(Option<BlobRef>, u64)>> {
        let row: Option<(Option<Vec<u8>>, u64)> = self
            .db
            .query_row(
                "SELECT blob, lamport FROM profile_pictures WHERE member = ?",
                [member],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(blob, lamport)| {
            let picture = blob
                .map(|bytes| postcard::from_bytes(&bytes))
                .transpose()
                .context("reading a stored picture")?;
            Ok((picture, lamport))
        })
        .transpose()
    }

    fn request_blob_removal(&self, author: &[u8], blob_id: &str, requested_by: &[u8]) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO blob_removals (author, blob_id, requested_by) VALUES (?1, ?2, ?3)",
            params![author, blob_id, requested_by],
        )?;
        Ok(())
    }

    pub fn blob_removals(&self) -> Result<Vec<BlobRemoval>> {
        self.db
            .prepare("SELECT author, blob_id, requested_by FROM blob_removals")?
            .query_map([], |row| {
                Ok(BlobRemoval {
                    author: row.get(0)?,
                    blob_id: row.get(1)?,
                    requested_by: row.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()
            .map_err(Into::into)
    }

    pub fn forget_blob_removal(&self, removal: &BlobRemoval) -> Result<()> {
        self.db.execute(
            "DELETE FROM blob_removals WHERE author = ? AND blob_id = ? AND requested_by = ?",
            params![removal.author, removal.blob_id, removal.requested_by],
        )?;
        Ok(())
    }
}

fn stored_file(row: &Row<'_>) -> rusqlite::Result<StoredFile> {
    let blob: Vec<u8> = row.get(7)?;
    let blob = postcard::from_bytes(&blob).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Blob, Box::new(error))
    })?;
    Ok(StoredFile {
        id: row.get(0)?,
        place: row.get(1)?,
        message: row.get(2)?,
        author: row.get(3)?,
        name: row.get(4)?,
        media_type: row.get(5)?,
        created_ms: row.get(6)?,
        blob,
    })
}
