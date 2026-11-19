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
}
