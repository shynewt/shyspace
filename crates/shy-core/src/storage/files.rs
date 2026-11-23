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
    /// leaves the bucket, through its author's device, unless it is the one
    /// still shown; the same picture arriving again changes nothing. Returns
    /// whether the picture shown changed.
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
        // The later clock wins; a tie, which only a forged op makes, goes to
        // the higher picture id, so every order ends the same.
        let id_of = |picture: Option<&BlobRef>| picture.map_or(String::new(), |picture| picture.id.clone());
        let incoming_id = id_of(picture);
        let kept_id = kept
            .as_ref()
            .map(|(kept_picture, _)| id_of(kept_picture.as_ref()));
        let is_newer = kept.as_ref().is_none_or(|(kept_picture, kept_lamport)| {
            (*kept_lamport, id_of(kept_picture.as_ref())) < (lamport, incoming_id.clone())
        });
        if !is_newer {
            if let Some(picture) = picture
                && kept_id.as_deref() != Some(picture.id.as_str())
            {
                self.request_blob_removal(member, &picture.id, member)?;
            }
            return Ok(false);
        }
        if let Some((Some(old), _)) = &kept
            && old.id != incoming_id
        {
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

    pub(super) fn request_blob_removal(
        &self,
        author: &[u8],
        blob_id: &str,
        requested_by: &[u8],
    ) -> Result<()> {
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
    use iroh::SecretKey;

    use crate::{
        files::{SharedFile, blob},
        ops::{Op, new_owned_id},
        storage::test_channel::TestChannel,
    };

    fn shared_file(channel: &TestChannel, message: Option<String>) -> SharedFile {
        let content = b"notes".to_vec();
        SharedFile {
            id: new_owned_id(&channel.author),
            place: channel.id.clone(),
            message,
            name: "notes.txt".into(),
            media_type: "text/plain".into(),
            blob: blob::seal(&mut content.as_slice(), content.len() as u64, |_, _, _| Ok(())).unwrap(),
        }
    }

    fn picture(content: &[u8]) -> crate::files::blob::BlobRef {
        blob::seal(&mut &content[..], content.len() as u64, |_, _, _| Ok(())).unwrap()
    }

    /// Only a forged op ties on lamport; still, every order shows the same
    /// picture, and only the other one leaves the bucket.
    #[test]
    fn pictures_tied_on_the_clock_settle_the_same_in_any_order() {
        let (first, second) = (picture(b"\x89PNG first"), picture(b"\x89PNG second"));
        let winner = first.id.clone().max(second.id.clone());
        let loser = first.id.clone().min(second.id.clone());
        for order in [[&first, &second], [&second, &first]] {
            let mut channel = TestChannel::new();
            channel.apply(Op::ProfilePicture {
                picture: Some(order[0].clone()),
            });
            channel.apply_again(Op::ProfilePicture {
                picture: Some(order[1].clone()),
            });
            let member = channel.author;

            let shown = channel.store.profile_picture(member.as_bytes()).unwrap().unwrap();
            let removed: Vec<String> = channel
                .store
                .blob_removals()
                .unwrap()
                .into_iter()
                .map(|removal| removal.blob_id)
                .collect();
            assert_eq!(shown.id, winner);
            assert_eq!(removed, std::slice::from_ref(&loser));
        }
    }

    /// The same blob id again (only a forged op reuses one) never sends the
    /// picture still shown out of the bucket.
    #[test]
    fn the_picture_shown_never_leaves_the_bucket() {
        let mut channel = TestChannel::new();
        let shown = picture(b"\x89PNG mine");
        channel.apply(Op::ProfilePicture {
            picture: Some(shown.clone()),
        });
        let mut same_id = picture(b"\x89PNG other");
        same_id.id = shown.id.clone();

        channel.apply(Op::ProfilePicture {
            picture: Some(same_id),
        });

        assert!(channel.store.blob_removals().unwrap().is_empty());
    }

    /// The delete can apply before the file: a rebuild or restore reads the
    /// bucket in a new order. The deleter's device still removes the pieces.
    #[test]
    fn a_file_that_arrives_after_its_delete_still_leaves_the_bucket() {
        let mut channel = TestChannel::new();
        let admin = SecretKey::generate().public();
        let file = shared_file(&channel, None);
        let message = new_owned_id(&channel.author);
        let file_of_message = shared_file(&channel, Some(message.clone()));
        channel.apply_as(admin, Op::Delete { id: file.id.clone() });
        channel.apply_as(admin, Op::Delete { id: message });

        channel.apply(Op::File { file: file.clone() });
        channel.apply(Op::File {
            file: file_of_message.clone(),
        });

        let mut removals: Vec<(Vec<u8>, String, Vec<u8>)> = channel
            .store
            .blob_removals()
            .unwrap()
            .into_iter()
            .map(|removal| (removal.author, removal.blob_id, removal.requested_by))
            .collect();
        removals.sort();
        let bob = channel.author.as_bytes().to_vec();
        let mut expected = vec![
            (bob.clone(), file.blob.id, admin.as_bytes().to_vec()),
            (bob, file_of_message.blob.id, admin.as_bytes().to_vec()),
        ];
        expected.sort();
        assert_eq!(removals, expected);
        assert!(
            channel
                .store
                .files_of_message(&file_of_message.message.unwrap(), &channel.id)
                .unwrap()
                .is_empty()
        );
    }

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
