//! The database schema. Until the first release it changes in place: there
//! are no migrations, and a store made by a different build is refused
//! ("recreate this workspace") rather than upgraded. That holds for the ops
//! in the log too: changing how an op is encoded (adding, removing or
//! reordering kinds) also needs a fresh workspace, so raise `VIEW_RULES`.

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

const SCHEMA: &str = r#"
CREATE TABLE meta (key TEXT PRIMARY KEY, value BLOB NOT NULL);
CREATE TABLE epoch_keys (epoch INTEGER PRIMARY KEY, key BLOB NOT NULL);
CREATE TABLE epoch_members (epoch INTEGER NOT NULL, member BLOB NOT NULL, PRIMARY KEY (epoch, member));
CREATE TABLE members (member BLOB PRIMARY KEY, active INTEGER NOT NULL);
CREATE TABLE commits (from_epoch INTEGER PRIMARY KEY, signed BLOB NOT NULL);
CREATE TABLE ops (
    author BLOB NOT NULL, seq INTEGER NOT NULL, epoch INTEGER NOT NULL,
    envelope BLOB NOT NULL, pushed INTEGER NOT NULL,
    PRIMARY KEY (author, seq));
CREATE TABLE pending_ops (
    hash BLOB PRIMARY KEY, author BLOB NOT NULL, seq INTEGER NOT NULL, epoch INTEGER NOT NULL,
    envelope BLOB NOT NULL);
CREATE TABLE profiles (
    member BLOB PRIMARY KEY, name TEXT NOT NULL, is_bot INTEGER NOT NULL, lamport INTEGER NOT NULL);
CREATE TABLE channels (
    id TEXT PRIMARY KEY, name TEXT NOT NULL, created_ms INTEGER NOT NULL,
    lamport INTEGER NOT NULL, author BLOB NOT NULL);
CREATE TABLE messages (
    id TEXT PRIMARY KEY, channel TEXT NOT NULL, author BLOB NOT NULL, text TEXT NOT NULL,
    reply_to TEXT, lamport INTEGER NOT NULL, created_ms INTEGER NOT NULL, seq INTEGER);
CREATE INDEX messages_by_channel ON messages (channel, lamport, author);
CREATE TABLE reactions (
    message TEXT NOT NULL, author BLOB NOT NULL, emoji TEXT NOT NULL,
    active INTEGER NOT NULL, lamport INTEGER NOT NULL, PRIMARY KEY (message, author, emoji));
CREATE TABLE pages (
    id TEXT PRIMARY KEY, parent TEXT, title TEXT NOT NULL DEFAULT '',
    title_lamport INTEGER NOT NULL DEFAULT 0, title_author BLOB,
    created_ms INTEGER, create_lamport INTEGER, create_author BLOB,
    updated_ms INTEGER NOT NULL DEFAULT 0, doc BLOB);
CREATE VIRTUAL TABLE search USING fts5(
    kind UNINDEXED, ref UNINDEXED, title, body, tokenize = 'unicode61 remove_diacritics 2');
CREATE TABLE frozen_authors (member BLOB PRIMARY KEY, last_seq INTEGER NOT NULL);
CREATE TABLE invites (
    token TEXT PRIMARY KEY, label TEXT NOT NULL, created_ms INTEGER NOT NULL, expires_ms INTEGER NOT NULL,
    state TEXT NOT NULL, joiner BLOB, joiner_name TEXT, joiner_is_bot INTEGER, request BLOB);
CREATE TABLE deliveries (joiner BLOB PRIMARY KEY, token TEXT NOT NULL, payload BLOB NOT NULL);
CREATE TABLE message_edits (message TEXT PRIMARY KEY, text TEXT NOT NULL, lamport INTEGER NOT NULL);
CREATE TABLE channel_names (
    channel TEXT PRIMARY KEY, name TEXT NOT NULL, lamport INTEGER NOT NULL, author BLOB NOT NULL);
CREATE TABLE deleted (id TEXT PRIMARY KEY, deleter BLOB);
CREATE TABLE read_markers (channel TEXT PRIMARY KEY, lamport INTEGER NOT NULL, author BLOB NOT NULL);
CREATE TABLE channel_notify (channel TEXT PRIMARY KEY, level TEXT NOT NULL);
CREATE TABLE thread_follows (root TEXT PRIMARY KEY, is_following INTEGER NOT NULL);
CREATE TABLE drafts (target TEXT PRIMARY KEY, text TEXT NOT NULL, updated_ms INTEGER NOT NULL);
CREATE TABLE later (
    item TEXT PRIMARY KEY, saved_ms INTEGER NOT NULL, remind_ms INTEGER,
    is_reminded INTEGER NOT NULL DEFAULT 0, is_done INTEGER NOT NULL DEFAULT 0);
CREATE TABLE sidebar_sections (
    id TEXT PRIMARY KEY, name TEXT NOT NULL, position INTEGER NOT NULL,
    sort TEXT NOT NULL DEFAULT 'name', is_collapsed INTEGER NOT NULL DEFAULT 0);
CREATE TABLE sidebar_placement (channel TEXT PRIMARY KEY, section TEXT NOT NULL);
CREATE TABLE member_status (
    member BLOB PRIMARY KEY, emoji TEXT NOT NULL, text TEXT NOT NULL, until_ms INTEGER,
    alerts INTEGER NOT NULL, lamport INTEGER NOT NULL);
CREATE TABLE sealing_secrets (public BLOB PRIMARY KEY, secret BLOB NOT NULL, created_ms INTEGER NOT NULL);
CREATE TABLE sealing_keys (member BLOB PRIMARY KEY, public BLOB NOT NULL, lamport INTEGER NOT NULL);
CREATE TABLE private_channels (
    id TEXT PRIMARY KEY, kind TEXT NOT NULL, origin_author BLOB NOT NULL, origin_seq INTEGER NOT NULL);
CREATE TABLE private_generations (
    channel TEXT NOT NULL, generation INTEGER NOT NULL, commitment BLOB NOT NULL,
    origin_author BLOB NOT NULL, origin_seq INTEGER NOT NULL, PRIMARY KEY (channel, generation));
CREATE TABLE private_keys (
    channel TEXT NOT NULL, generation INTEGER NOT NULL, key BLOB NOT NULL,
    origin_author BLOB NOT NULL, origin_seq INTEGER NOT NULL, PRIMARY KEY (channel, generation, key));
CREATE TABLE private_adds (
    channel TEXT NOT NULL, member BLOB NOT NULL, after_seq INTEGER NOT NULL, from_generation INTEGER NOT NULL,
    origin_author BLOB NOT NULL, origin_seq INTEGER NOT NULL, PRIMARY KEY (origin_author, origin_seq, member));
CREATE INDEX private_adds_by_member ON private_adds (channel, member);
CREATE TABLE dm_members (channel TEXT NOT NULL, member BLOB NOT NULL, PRIMARY KEY (channel, member));
CREATE TABLE sealed_pending (
    author BLOB NOT NULL, seq INTEGER NOT NULL, hint BLOB NOT NULL, channel TEXT, PRIMARY KEY (author, seq));
CREATE TABLE private_applied (
    channel TEXT NOT NULL, author BLOB NOT NULL, seq INTEGER NOT NULL, generation INTEGER, PRIMARY KEY (author, seq));
CREATE INDEX private_applied_by_channel ON private_applied (channel, author, seq);
CREATE TABLE private_owners (
    channel TEXT NOT NULL, from_position INTEGER NOT NULL, owner BLOB NOT NULL,
    origin_author BLOB NOT NULL, origin_seq INTEGER NOT NULL, PRIMARY KEY (channel, from_position));
CREATE TABLE private_positions (
    channel TEXT NOT NULL, position INTEGER NOT NULL,
    origin_author BLOB NOT NULL, origin_seq INTEGER NOT NULL, PRIMARY KEY (channel, position));
CREATE TABLE private_closures (
    channel TEXT NOT NULL, member BLOB NOT NULL, at_seq INTEGER NOT NULL, before_generation INTEGER NOT NULL,
    is_leave INTEGER NOT NULL, origin_author BLOB NOT NULL, origin_seq INTEGER NOT NULL,
    PRIMARY KEY (origin_author, origin_seq));
CREATE INDEX private_closures_by_member ON private_closures (channel, member);
CREATE TABLE forks (author BLOB NOT NULL, seq INTEGER NOT NULL, proof BLOB NOT NULL, PRIMARY KEY (author, seq));
CREATE TABLE fork_proofs_checked (path TEXT PRIMARY KEY, waiting_for_epoch INTEGER);
CREATE TABLE profile_details (
    member BLOB PRIMARY KEY, title TEXT NOT NULL, pronouns TEXT NOT NULL, bio TEXT NOT NULL,
    email TEXT NOT NULL, phone TEXT NOT NULL, website TEXT NOT NULL, location TEXT NOT NULL,
    time_zone TEXT NOT NULL, work_days INTEGER, work_start INTEGER, work_end INTEGER,
    lamport INTEGER NOT NULL);
CREATE TABLE files (
    id TEXT PRIMARY KEY, place TEXT NOT NULL, message TEXT, author BLOB NOT NULL, name TEXT NOT NULL,
    media_type TEXT NOT NULL, size INTEGER NOT NULL, blob BLOB NOT NULL, created_ms INTEGER NOT NULL,
    seq INTEGER NOT NULL);
CREATE INDEX files_by_place ON files (place, created_ms);
CREATE INDEX files_by_message ON files (message);
CREATE TABLE profile_pictures (member BLOB PRIMARY KEY, blob BLOB, lamport INTEGER NOT NULL);
CREATE TABLE blob_removals (
    author BLOB NOT NULL, blob_id TEXT NOT NULL, requested_by BLOB NOT NULL,
    PRIMARY KEY (author, blob_id, requested_by));
CREATE TABLE file_names (file TEXT PRIMARY KEY, name TEXT NOT NULL, lamport INTEGER NOT NULL);
CREATE TABLE search_rows (
    kind TEXT NOT NULL, ref TEXT NOT NULL, search_rowid INTEGER NOT NULL,
    PRIMARY KEY (kind, ref)) WITHOUT ROWID;
CREATE INDEX messages_by_reply_to ON messages (reply_to, channel);
-- The built-in section every channel starts in.
INSERT INTO sidebar_sections (id, name, position) VALUES ('channels', 'Channels', 0);
"#;

/// Raised when the rules for showing ops change (what a view holds, not its
/// tables) or how ops are encoded, so a store made under other rules is
/// refused like one with another schema.
const VIEW_RULES: u8 = 1;

/// What a store records about the build that made it: changes whenever the
/// schema or the view rules do. Never 0, which means a new store.
fn store_version() -> i32 {
    let hash = blake3::Hasher::new()
        .update(SCHEMA.as_bytes())
        .update(&[VIEW_RULES])
        .finalize();
    let mut first = [0; 4];
    first.copy_from_slice(&hash.as_bytes()[..4]);
    match i32::from_le_bytes(first) {
        0 => 1,
        version => version,
    }
}

/// Sets up a new store, or checks that an existing one was made by this
/// build's schema and rules.
pub(super) fn create_or_check(db: &mut Connection) -> Result<()> {
    let version: i32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == store_version() {
        return Ok(());
    }
    let tables: i64 = db.query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))?;
    if version != 0 || tables > 0 {
        bail!("this workspace was made by a different build of shyspace; recreate this workspace");
    }
    let transaction = db.transaction()?;
    transaction
        .execute_batch(SCHEMA)
        .context("creating the database")?;
    transaction.pragma_update(None, "user_version", store_version())?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_store_gets_the_schema_once() {
        let mut db = Connection::open_in_memory().unwrap();
        create_or_check(&mut db).unwrap();
        create_or_check(&mut db).unwrap();
        let tables: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(tables > 40, "{tables} tables");
    }

    #[test]
    fn a_store_from_another_build_is_refused_clearly() {
        let mut db = Connection::open_in_memory().unwrap();
        create_or_check(&mut db).unwrap();
        db.pragma_update(None, "user_version", store_version().wrapping_add(1))
            .unwrap();

        let error = create_or_check(&mut db).unwrap_err();

        assert!(error.to_string().contains("recreate this workspace"), "{error}");
    }

    #[test]
    fn a_store_with_tables_but_no_version_is_refused_clearly() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value BLOB NOT NULL);")
            .unwrap();

        let error = create_or_check(&mut db).unwrap_err();

        assert!(error.to_string().contains("recreate this workspace"), "{error}");
    }
}
