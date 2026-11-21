//! Versioned schema. Each migration runs once, in order, tracked by SQLite's
//! `user_version`. Never edit a released migration; append a new one.

use anyhow::{Context, Result, ensure};
use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    "
CREATE TABLE meta (key TEXT PRIMARY KEY, value BLOB NOT NULL);
CREATE TABLE epoch_keys (epoch INTEGER PRIMARY KEY, key BLOB NOT NULL);
CREATE TABLE epoch_members (epoch INTEGER NOT NULL, member BLOB NOT NULL, PRIMARY KEY (epoch, member));
CREATE TABLE members (member BLOB PRIMARY KEY, active INTEGER NOT NULL);
CREATE TABLE deliveries (joiner BLOB PRIMARY KEY, payload BLOB NOT NULL);
CREATE TABLE commits (from_epoch INTEGER PRIMARY KEY, signed BLOB NOT NULL);
CREATE TABLE ops (
    author BLOB NOT NULL, seq INTEGER NOT NULL, epoch INTEGER NOT NULL,
    envelope BLOB NOT NULL, pushed INTEGER NOT NULL,
    PRIMARY KEY (author, seq));
CREATE TABLE pending_ops (
    hash BLOB PRIMARY KEY, author BLOB NOT NULL, seq INTEGER NOT NULL, epoch INTEGER NOT NULL,
    envelope BLOB NOT NULL);
CREATE TABLE cursors (author BLOB PRIMARY KEY, last_key TEXT NOT NULL);
CREATE TABLE profiles (
    member BLOB PRIMARY KEY, name TEXT NOT NULL, is_bot INTEGER NOT NULL, lamport INTEGER NOT NULL);
CREATE TABLE channels (
    id TEXT PRIMARY KEY, name TEXT NOT NULL, created_ms INTEGER NOT NULL,
    lamport INTEGER NOT NULL, author BLOB NOT NULL);
CREATE TABLE messages (
    id TEXT PRIMARY KEY, channel TEXT NOT NULL, author BLOB NOT NULL, text TEXT NOT NULL,
    reply_to TEXT, lamport INTEGER NOT NULL, created_ms INTEGER NOT NULL);
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
",
    "
CREATE TABLE frozen_authors (member BLOB PRIMARY KEY, last_seq INTEGER NOT NULL);
",
    "
CREATE TABLE invites (
    token TEXT PRIMARY KEY, label TEXT NOT NULL, created_ms INTEGER NOT NULL, expires_ms INTEGER NOT NULL,
    state TEXT NOT NULL, joiner BLOB, joiner_name TEXT, joiner_is_bot INTEGER, request BLOB);
DROP TABLE deliveries;
CREATE TABLE deliveries (joiner BLOB PRIMARY KEY, token TEXT NOT NULL, payload BLOB NOT NULL);
",
    "
CREATE TABLE message_edits (message TEXT PRIMARY KEY, text TEXT NOT NULL, lamport INTEGER NOT NULL);
CREATE TABLE channel_names (
    channel TEXT PRIMARY KEY, name TEXT NOT NULL, lamport INTEGER NOT NULL, author BLOB NOT NULL);
CREATE TABLE deleted (id TEXT PRIMARY KEY);
",
    "
CREATE TABLE read_markers (channel TEXT PRIMARY KEY, lamport INTEGER NOT NULL, author BLOB NOT NULL);
",
    "
CREATE TABLE channel_notify (channel TEXT PRIMARY KEY, level TEXT NOT NULL);
",
    "
CREATE TABLE thread_follows (root TEXT PRIMARY KEY, is_following INTEGER NOT NULL);
",
    "
CREATE TABLE drafts (target TEXT PRIMARY KEY, text TEXT NOT NULL, updated_ms INTEGER NOT NULL);
",
    "
CREATE TABLE later (
    item TEXT PRIMARY KEY, saved_ms INTEGER NOT NULL, remind_ms INTEGER,
    is_reminded INTEGER NOT NULL DEFAULT 0, is_done INTEGER NOT NULL DEFAULT 0);
",
    "
CREATE TABLE sidebar_sections (
    id TEXT PRIMARY KEY, name TEXT NOT NULL, position INTEGER NOT NULL,
    sort TEXT NOT NULL DEFAULT 'name', is_collapsed INTEGER NOT NULL DEFAULT 0);
INSERT INTO sidebar_sections (id, name, position) VALUES ('channels', 'Channels', 0);
CREATE TABLE sidebar_placement (channel TEXT PRIMARY KEY, section TEXT NOT NULL);
",
    "
CREATE TABLE member_status (
    member BLOB PRIMARY KEY, emoji TEXT NOT NULL, text TEXT NOT NULL, until_ms INTEGER,
    is_busy INTEGER NOT NULL, lamport INTEGER NOT NULL);
",
    // 0 = all, 1 = nothing (the old busy flag), 2 = mentions.
    "ALTER TABLE member_status RENAME COLUMN is_busy TO alerts;",
    "
CREATE TABLE sealing_secrets (public BLOB PRIMARY KEY, secret BLOB NOT NULL, created_ms INTEGER NOT NULL);
CREATE TABLE sealing_keys (member BLOB PRIMARY KEY, public BLOB NOT NULL, lamport INTEGER NOT NULL);
",
    // Private channels. Rows record the op that made them (origin), so a
    // removed member's late ops can be taken back out.
    "
CREATE TABLE private_channels (
    id TEXT PRIMARY KEY, kind TEXT NOT NULL, origin_author BLOB NOT NULL, origin_seq INTEGER NOT NULL);
CREATE TABLE private_owners (
    channel TEXT NOT NULL, position INTEGER NOT NULL, owner BLOB NOT NULL,
    origin_author BLOB NOT NULL, origin_seq INTEGER NOT NULL, PRIMARY KEY (channel, position));
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
CREATE TABLE private_closures (
    channel TEXT NOT NULL, member BLOB NOT NULL, at_seq INTEGER NOT NULL, before_generation INTEGER NOT NULL,
    origin_author BLOB NOT NULL, origin_seq INTEGER NOT NULL, PRIMARY KEY (origin_author, origin_seq));
CREATE INDEX private_closures_by_member ON private_closures (channel, member);
CREATE TABLE dm_members (channel TEXT NOT NULL, member BLOB NOT NULL, PRIMARY KEY (channel, member));
CREATE TABLE sealed_pending (
    author BLOB NOT NULL, seq INTEGER NOT NULL, hint BLOB NOT NULL, channel TEXT, PRIMARY KEY (author, seq));
CREATE TABLE private_applied (
    channel TEXT NOT NULL, author BLOB NOT NULL, seq INTEGER NOT NULL, PRIMARY KEY (author, seq));
CREATE INDEX private_applied_by_channel ON private_applied (channel, author, seq);
",
    // Owner ops form one numbered log per channel; closures say whether they
    // were a leave. Nothing used the earlier tables yet.
    "
DROP TABLE private_owners;
CREATE TABLE private_owners (
    channel TEXT NOT NULL, from_position INTEGER NOT NULL, owner BLOB NOT NULL,
    origin_author BLOB NOT NULL, origin_seq INTEGER NOT NULL, PRIMARY KEY (channel, from_position));
CREATE TABLE private_positions (
    channel TEXT NOT NULL, position INTEGER NOT NULL,
    origin_author BLOB NOT NULL, origin_seq INTEGER NOT NULL, PRIMARY KEY (channel, position));
DROP TABLE private_closures;
CREATE TABLE private_closures (
    channel TEXT NOT NULL, member BLOB NOT NULL, at_seq INTEGER NOT NULL, before_generation INTEGER NOT NULL,
    is_leave INTEGER NOT NULL, origin_author BLOB NOT NULL, origin_seq INTEGER NOT NULL,
    PRIMARY KEY (origin_author, origin_seq));
CREATE INDEX private_closures_by_member ON private_closures (channel, member);
DELETE FROM private_channels; DELETE FROM private_generations; DELETE FROM private_keys;
DELETE FROM private_adds; DELETE FROM dm_members; DELETE FROM sealed_pending; DELETE FROM private_applied;
",
    // Where a message sits in its author's log: their earliest message with
    // an id wins. Older rows have none and stay as they are.
    "ALTER TABLE messages ADD COLUMN seq INTEGER;",
    // Op slots an author signed twice, with the proof; neither version counts.
    // Fork proofs from the bucket already checked: refused, or waiting for an
    // epoch key.
    "
CREATE TABLE forks (author BLOB NOT NULL, seq INTEGER NOT NULL, proof BLOB NOT NULL, PRIMARY KEY (author, seq));
CREATE TABLE fork_proofs_checked (path TEXT PRIMARY KEY, waiting_for_epoch INTEGER);
",
    // Working hours are all three work_ columns, or none.
    "
CREATE TABLE profile_details (
    member BLOB PRIMARY KEY, title TEXT NOT NULL, pronouns TEXT NOT NULL, bio TEXT NOT NULL,
    email TEXT NOT NULL, phone TEXT NOT NULL, website TEXT NOT NULL, location TEXT NOT NULL,
    time_zone TEXT NOT NULL, work_days INTEGER, work_start INTEGER, work_end INTEGER,
    lamport INTEGER NOT NULL);
",
];

pub(super) fn migrate(db: &mut Connection) -> Result<()> {
    let applied: usize = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    ensure!(
        applied <= MIGRATIONS.len(),
        "this database was created by a newer version of shyspace"
    );
    for (index, migration) in MIGRATIONS.iter().enumerate().skip(applied) {
        let transaction = db.transaction()?;
        transaction
            .execute_batch(migration)
            .with_context(|| format!("database migration {}", index + 1))?;
        transaction.pragma_update(None, "user_version", index + 1)?;
        transaction.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_once_and_reject_newer_databases() {
        let mut db = Connection::open_in_memory().unwrap();
        migrate(&mut db).unwrap();
        migrate(&mut db).unwrap();
        db.pragma_update(None, "user_version", MIGRATIONS.len() + 1)
            .unwrap();
        assert!(migrate(&mut db).is_err());
    }

    #[test]
    fn a_database_from_before_profile_details_upgrades() {
        let mut db = Connection::open_in_memory().unwrap();
        let before = MIGRATIONS.len() - 1;
        for migration in &MIGRATIONS[..before] {
            db.execute_batch(migration).unwrap();
        }
        db.pragma_update(None, "user_version", before).unwrap();
        db.execute(
            "INSERT INTO forks (author, seq, proof) VALUES (x'01', 1, x'02')",
            [],
        )
        .unwrap();

        migrate(&mut db).unwrap();
        let kept: i64 = db
            .query_row("SELECT COUNT(*) FROM forks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(kept, 1);
        let details: i64 = db
            .query_row("SELECT COUNT(*) FROM profile_details", [], |row| row.get(0))
            .unwrap();
        assert_eq!(details, 0);
    }
}
