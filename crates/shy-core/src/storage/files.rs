//! Shared files and profile pictures, and the bucket removals they leave
//! behind. A blob stays in the bucket until its file is deleted or its
//! picture replaced; then the device that deleted or replaced it takes it
//! out (see `workspace/files.rs`).

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, Row, named_params, params};

use super::Store;
use crate::{
    files::{SharedFile, blob::BlobRef, clean_file_name, clean_media_type},
    ops::VerifiedOp,
    views::Change,
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

/// A file with its latest name.
const FILE_COLUMNS: &str =
    "f.id, f.place, f.message, f.author, COALESCE(n.name, f.name), f.media_type, f.created_ms, f.blob";
const FILES: &str = "files f LEFT JOIN file_names n ON n.file = f.id";

impl Store {
    /// Keeps a file from someone else, cleaned. For an id used twice, the
    /// op earliest in its author's log wins, whatever order they opened in.
    pub(super) fn put_file(&self, file: &SharedFile, verified: &VerifiedOp) -> Result<()> {
        self.db.execute(
            "INSERT INTO files (id, place, message, author, name, media_type, size, blob, created_ms, seq)
             VALUES (:id, :place, :message, :author, :name, :media_type, :size, :blob, :now, :seq)
             ON CONFLICT (id) DO UPDATE SET place = :place, message = :message, name = :name,
                 media_type = :media_type, size = :size, blob = :blob, created_ms = :now, seq = :seq
             WHERE :seq < seq",
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
                ":seq": verified.seq,
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
            self.db
                .execute("DELETE FROM file_names WHERE file = ?", [&file])?;
            self.request_blob_removal(&author, &blob.id, deleter)?;
            if !places.contains(&place) {
                places.push(place);
            }
        }
        Ok(places)
    }

    /// Keeps a rename from the file's sharer, cleaned, unless a later one
    /// is kept. The file may not have arrived yet; it shows the name when it does.
    pub(super) fn apply_file_rename(&self, id: &str, name: &str, lamport: u64) -> Result<Option<Change>> {
        let changed = self.db.execute(
            "INSERT INTO file_names (file, name, lamport) VALUES (?1, ?2, ?3)
             ON CONFLICT (file) DO UPDATE SET name = ?2, lamport = ?3 WHERE lamport < ?3",
            params![id, clean_file_name(name), lamport],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        let place: Option<String> = self
            .db
            .query_row("SELECT place FROM files WHERE id = ?", [id], |row| row.get(0))
            .optional()?;
        Ok(place.map(|channel| Change::Messages { channel }))
    }

    pub fn file(&self, id: &str) -> Result<Option<StoredFile>> {
        Ok(self
            .db
            .query_row(
                &format!("SELECT {FILE_COLUMNS} FROM {FILES} WHERE f.id = ?"),
                [id],
                stored_file,
            )
            .optional()?)
    }

    /// The files shared with `message` in `channel`, oldest first.
    pub fn files_of_message(&self, message: &str, channel: &str) -> Result<Vec<StoredFile>> {
        self.db
            .prepare(&format!(
                "SELECT {FILE_COLUMNS} FROM {FILES} WHERE f.message = ? AND f.place = ? ORDER BY f.created_ms, f.id"
            ))?
            .query_map([message, channel], stored_file)?
            .collect::<Result<_, _>>()
            .map_err(Into::into)
    }

    /// Where the kept file with this id sits in its author's log.
    pub(super) fn file_seq(&self, id: &str) -> Result<Option<u64>> {
        Ok(self
            .db
            .query_row("SELECT seq FROM files WHERE id = ?", [id], |row| row.get(0))
            .optional()?)
    }

    /// Every file this device can see, newest first.
    pub fn all_files(&self) -> Result<Vec<StoredFile>> {
        self.db
            .prepare(&format!(
                "SELECT {FILE_COLUMNS} FROM {FILES} ORDER BY f.created_ms DESC, f.id"
            ))?
            .query_map([], stored_file)?
            .collect::<Result<_, _>>()
            .map_err(Into::into)
    }

    /// Keeps a member's picture unless a newer one is kept. Whichever loses
    /// leaves the bucket, through its author's device; the same picture
    /// arriving again changes nothing. Returns whether the picture shown
    /// changed.
    pub(super) fn put_profile_picture(
        &self,
        member: &[u8],
        picture: Option<&BlobRef>,
        lamport: u64,
    ) -> Result<bool> {
        let kept = self.kept_picture(member)?;
        if kept.as_ref().is_some_and(|(kept_picture, kept_lamport)| {
            *kept_lamport == lamport && kept_picture.as_ref() == picture
        }) {
            return Ok(false);
        }
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

#[cfg(test)]
mod tests {
    use crate::{files::blob, ops::Op, storage::test_channel::TestChannel};

    #[test]
    fn applying_a_picture_twice_changes_nothing() {
        let mut channel = TestChannel::new();
        let content = b"\x89PNG\r\n\x1a\npicture".to_vec();
        let picture = blob::seal(&mut content.as_slice(), content.len() as u64, |_, _, _| Ok(())).unwrap();
        let op = Op::ProfilePicture {
            picture: Some(picture.clone()),
        };
        channel.apply(op.clone());
        channel.apply_again(op);

        let author = channel.author;
        assert!(
            channel.store.blob_removals().unwrap().is_empty(),
            "the shown picture stays"
        );
        assert_eq!(
            channel
                .store
                .profile_picture(author.as_bytes())
                .unwrap()
                .map(|kept| kept.id),
            Some(picture.id)
        );
    }
}
