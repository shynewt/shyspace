//! The database schema, as a list of migrations: a store's `user_version`
//! is how many it has had. Stored data is kept from v0.1.0 on,
//! so a change is a new migration at the end (additive), never an edit to
//! one already released. A store from a newer app is refused ("update
//! shyspace"): everyone moves to new versions together. So is one whose
//! views or log a newer app wrote under newer view rules or op format,
//! which change without a migration. One from a build
//! that never shipped can't be read by any release, so it is refused as
//! such. Migrations run in one transaction, so they can't use `VACUUM` or `PRAGMA foreign_keys`.

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension};

use crate::crypto::OP_FORMAT;

/// Every migration in order; the first is the schema as of v0.1.0.
const MIGRATIONS: &[&str] = &[
    FIRST_SCHEMA,
    WORKSPACE_LOOK,
    PAGE_MENTIONS,
    DEVICES,
    DEVICE_CHANGES,
    DEVICE_ACTIONS,
    PENDING_LINKS,
];

/// The workspace's icon and default look, set by the admin.
const WORKSPACE_LOOK: &str = r#"
CREATE TABLE workspace_icon (id INTEGER PRIMARY KEY CHECK (id = 1), blob BLOB, lamport INTEGER NOT NULL);
CREATE TABLE workspace_look (id INTEGER PRIMARY KEY CHECK (id = 1), look BLOB NOT NULL, lamport INTEGER NOT NULL);
"#;

/// Unread mentions of you in pages, one per page and author; this device only.
const PAGE_MENTIONS: &str = r#"
CREATE TABLE page_mentions (
    page TEXT NOT NULL, author BLOB NOT NULL, position BLOB NOT NULL, preview TEXT NOT NULL,
    mentioned_ms INTEGER NOT NULL, is_read INTEGER NOT NULL,
    PRIMARY KEY (page, author));
"#;

/// Devices linked to a person, and what the views will need to tell a
/// person's devices apart. `frozen_epoch` is set only by commits that bind
/// the device map.
const DEVICES: &str = r#"
CREATE TABLE devices (
    device BLOB PRIMARY KEY, person BLOB NOT NULL, voucher BLOB NOT NULL,
    certificate BLOB NOT NULL, added_epoch INTEGER NOT NULL,
    from_grant INTEGER NOT NULL, added_ms INTEGER NOT NULL);
CREATE INDEX devices_by_person ON devices (person);
ALTER TABLE message_edits ADD COLUMN device BLOB;
ALTER TABLE file_names ADD COLUMN device BLOB;
ALTER TABLE frozen_authors ADD COLUMN frozen_epoch INTEGER;
"#;

/// Device changes every member judged and kept, a view rebuilt from ops.
const DEVICE_CHANGES: &str = r#"
CREATE TABLE device_changes (
    author BLOB NOT NULL, seq INTEGER NOT NULL, epoch INTEGER NOT NULL, person BLOB NOT NULL,
    change BLOB NOT NULL, shown_ms INTEGER NOT NULL,
    PRIMARY KEY (author, seq));
CREATE INDEX device_changes_by_person ON device_changes (person);
"#;

/// The admin's record of acting on device changes, and what each still
/// owes; not views, so the recovery snapshot keeps them.
const DEVICE_ACTIONS: &str = r#"
CREATE TABLE device_changes_acted (
    author BLOB NOT NULL, seq INTEGER NOT NULL, acted TEXT NOT NULL, acted_ms INTEGER NOT NULL,
    PRIMARY KEY (author, seq));
CREATE TABLE link_deliveries (
    device BLOB PRIMARY KEY, token BLOB NOT NULL, welcome BLOB NOT NULL, map_digest BLOB NOT NULL,
    epoch INTEGER NOT NULL, devices_before BLOB NOT NULL, sealed_grant BLOB, until_ms INTEGER NOT NULL);
CREATE TABLE pending_outcomes (
    token BLOB NOT NULL, device BLOB NOT NULL, outcome BLOB NOT NULL,
    PRIMARY KEY (token, device));
"#;

/// Links this device confirmed and vouched for, until they are acted on.
const PENDING_LINKS: &str = r#"
CREATE TABLE pending_links (
    token BLOB PRIMARY KEY, code BLOB NOT NULL, request BLOB NOT NULL, certificate BLOB NOT NULL,
    answer_until_ms INTEGER NOT NULL, handed_over INTEGER NOT NULL, refused INTEGER NOT NULL,
    slots_deleted INTEGER NOT NULL, until_ms INTEGER NOT NULL);
CREATE TABLE device_platforms (device BLOB PRIMARY KEY, platform BLOB NOT NULL);
CREATE TABLE device_notices (
    device BLOB NOT NULL, kind INTEGER NOT NULL, at_ms INTEGER NOT NULL, seen INTEGER NOT NULL,
    PRIMARY KEY (device, kind));
"#;

const FIRST_SCHEMA: &str = r#"
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

/// Raised when the rules for showing ops change (what a view holds, not
/// its tables), or a new kind of op is added: a store whose views were built
/// under other rules has them rebuilt from its log on opening, which also
/// applies ops of new kinds that an older version kept as unreadable. Rules
/// 2 add the workspace icon and look; rules 3 let any device of a person
/// edit their content, with equal clocks settled by device key; rules 4 add
/// device changes.
pub(crate) const VIEW_RULES: u8 = 4;
pub(crate) const VIEW_RULES_META: &str = "view_rules";
/// The newest op format any version that opened the store reads, as a
/// little-endian `u32`: a store whose log may hold ops in a format we can't
/// read is refused rather than half read.
pub(crate) const OP_FORMAT_META: &str = "op_format";
const NEWER_VERSION: &str = "this workspace was opened by a newer version of shyspace; update shyspace";

/// What stores made before migrations recorded instead of a count: a hash
/// of the first schema and the rules and op format of then. They hold
/// exactly the first migration, under view rules 1.
fn fingerprint_before_migrations() -> i32 {
    let hash = blake3::Hasher::new()
        .update(FIRST_SCHEMA.as_bytes())
        .update(&[1])
        .update(&1_u32.to_le_bytes())
        .finalize();
    let mut first = [0; 4];
    first.copy_from_slice(&hash.as_bytes()[..4]);
    match i32::from_le_bytes(first) {
        0 => 1,
        version => version,
    }
}

/// Above this, a `user_version` is no migration count but the fingerprint
/// of a build that never shipped.
const LARGEST_COUNT: i32 = 1_000;

/// Sets up a new store, or brings an existing one up to date.
pub(super) fn create_or_migrate(db: &mut Connection) -> Result<()> {
    migrate(db, MIGRATIONS)?;
    refuse_newer_rules(db)?;
    record_op_format(db)
}

/// View rules and op formats change without a migration, so a newer
/// version leaves them in `meta` for an older one to refuse.
fn refuse_newer_rules(db: &Connection) -> Result<()> {
    if let Some(rules) = stored(db, VIEW_RULES_META)? {
        let [rules] = rules.as_slice() else {
            anyhow::bail!("the stored view rules are damaged");
        };
        ensure!(*rules <= VIEW_RULES, NEWER_VERSION);
    }
    if let Some(format) = stored_op_format(db)? {
        ensure!(format <= OP_FORMAT, NEWER_VERSION);
    }
    Ok(())
}

fn record_op_format(db: &Connection) -> Result<()> {
    if stored_op_format(db)? == Some(OP_FORMAT) {
        return Ok(());
    }
    db.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2",
        rusqlite::params![OP_FORMAT_META, OP_FORMAT.to_le_bytes().as_slice()],
    )?;
    Ok(())
}

fn stored_op_format(db: &Connection) -> Result<Option<u32>> {
    stored(db, OP_FORMAT_META)?
        .map(|bytes| {
            let bytes: [u8; 4] = bytes
                .try_into()
                .map_err(|_| anyhow::anyhow!("the stored op format is damaged"))?;
            Ok(u32::from_le_bytes(bytes))
        })
        .transpose()
}

fn stored(db: &Connection, key: &str) -> Result<Option<Vec<u8>>> {
    Ok(db
        .query_row("SELECT value FROM meta WHERE key = ?", [key], |row| row.get(0))
        .optional()?)
}

fn migrate(db: &mut Connection, migrations: &[&str]) -> Result<()> {
    let mut applied: i32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if applied == fingerprint_before_migrations() {
        applied = 1;
    }
    let known = i32::try_from(migrations.len())?;
    ensure!(
        (0..=LARGEST_COUNT).contains(&applied),
        "this workspace was made by a different, unreleased build of shyspace; create it again"
    );
    ensure!(applied <= known, NEWER_VERSION);
    let tables: i64 = db.query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))?;
    ensure!(
        applied > 0 || tables == 0,
        "this database isn't a shyspace workspace, or is damaged"
    );
    let stored: i32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if stored == known {
        return Ok(());
    }
    let is_new = applied == 0;
    let transaction = db.transaction()?;
    for (index, migration) in migrations.iter().enumerate().skip(usize::try_from(applied)?) {
        transaction
            .execute_batch(migration)
            .with_context(|| format!("updating the database (step {})", index + 1))?;
    }
    if is_new {
        transaction.execute(
            "INSERT INTO meta (key, value) VALUES (?, ?)",
            rusqlite::params![VIEW_RULES_META, [VIEW_RULES].as_slice()],
        )?;
    }
    transaction.pragma_update(None, "user_version", known)?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(db: &Connection) -> i32 {
        db.pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn a_new_store_gets_the_schema_once() {
        let mut db = Connection::open_in_memory().unwrap();
        create_or_migrate(&mut db).unwrap();
        create_or_migrate(&mut db).unwrap();
        let tables: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(tables > 40, "{tables} tables");
        assert_eq!(version(&db), i32::try_from(MIGRATIONS.len()).unwrap());
    }

    #[test]
    fn a_store_takes_the_migrations_added_after_it_was_made() {
        let mut db = Connection::open_in_memory().unwrap();
        migrate(&mut db, &[FIRST_SCHEMA]).unwrap();
        db.execute("INSERT INTO meta (key, value) VALUES ('kept', x'01')", [])
            .unwrap();

        migrate(
            &mut db,
            &[FIRST_SCHEMA, "CREATE TABLE added (id TEXT PRIMARY KEY);"],
        )
        .unwrap();

        assert_eq!(version(&db), 2);
        db.execute("INSERT INTO added (id) VALUES ('new')", []).unwrap();
        let kept: i64 = db
            .query_row("SELECT COUNT(*) FROM meta WHERE key = 'kept'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(kept, 1, "what was stored is kept");
    }

    #[test]
    fn a_store_from_a_newer_app_is_refused_clearly() {
        let mut db = Connection::open_in_memory().unwrap();
        let mut newer = MIGRATIONS.to_vec();
        newer.push("CREATE TABLE added (id TEXT PRIMARY KEY);");
        migrate(&mut db, &newer).unwrap();

        let error = create_or_migrate(&mut db).unwrap_err();

        assert!(error.to_string().contains("update shyspace"), "{error}");
        assert_eq!(
            version(&db),
            i32::try_from(newer.len()).unwrap(),
            "left as it was"
        );
    }

    fn meta(db: &Connection, key: &str) -> Option<Vec<u8>> {
        db.query_row("SELECT value FROM meta WHERE key = ?", [key], |row| row.get(0))
            .ok()
    }

    fn set_meta(db: &Connection, key: &str, value: &[u8]) {
        db.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2",
            rusqlite::params![key, value],
        )
        .unwrap();
    }

    /// Newer view rules or op formats come without a migration, so the
    /// store records them and an older app refuses rather than rebuilding
    /// its views from ops it can't read.
    #[test]
    fn a_store_with_newer_views_or_ops_is_refused_clearly() {
        let newer_rules = [VIEW_RULES + 1];
        let newer_format = (OP_FORMAT + 1).to_le_bytes();
        for (key, value) in [
            (VIEW_RULES_META, newer_rules.as_slice()),
            (OP_FORMAT_META, newer_format.as_slice()),
        ] {
            let mut db = Connection::open_in_memory().unwrap();
            create_or_migrate(&mut db).unwrap();
            set_meta(&db, key, value);

            let error = create_or_migrate(&mut db).unwrap_err();

            assert!(error.to_string().contains("update shyspace"), "{key}: {error}");
            assert_eq!(meta(&db, key).as_deref(), Some(value), "left as it was");
        }
    }

    #[test]
    fn damaged_view_rules_or_op_format_are_refused() {
        for key in [VIEW_RULES_META, OP_FORMAT_META] {
            let mut db = Connection::open_in_memory().unwrap();
            create_or_migrate(&mut db).unwrap();
            set_meta(&db, key, &[1, 2, 3]);

            let error = create_or_migrate(&mut db).unwrap_err();

            assert!(error.to_string().contains("damaged"), "{key}: {error}");
        }
    }

    #[test]
    fn a_store_records_the_op_format_it_was_opened_with() {
        let mut db = Connection::open_in_memory().unwrap();
        create_or_migrate(&mut db).unwrap();
        db.execute("DELETE FROM meta WHERE key = ?", [OP_FORMAT_META])
            .unwrap();

        create_or_migrate(&mut db).unwrap();

        assert_eq!(meta(&db, OP_FORMAT_META), Some(OP_FORMAT.to_le_bytes().to_vec()));
    }

    #[test]
    fn a_store_from_an_unreleased_build_is_not_called_newer() {
        for unknown in [1_234_567_890, -1_234_567_890, -1] {
            let mut db = Connection::open_in_memory().unwrap();
            db.execute_batch(FIRST_SCHEMA).unwrap();
            db.pragma_update(None, "user_version", unknown).unwrap();

            let error = create_or_migrate(&mut db).unwrap_err();

            let message = error.to_string();
            assert!(!message.contains("update shyspace"), "{unknown}: {message}");
            assert!(message.contains("unreleased build"), "{unknown}: {message}");
            assert_eq!(version(&db), unknown, "left as it was");
        }
    }

    /// Stores made before migrations hold this number or a count: a fingerprint computed
    /// from the code alone would follow an edit to the first migration and
    /// stop matching them.
    #[test]
    fn the_fingerprint_is_the_one_the_previous_build_wrote() {
        assert_eq!(fingerprint_before_migrations(), 1_670_830_970);
    }

    #[test]
    fn released_migrations_are_never_edited() {
        let released = ["62e0a7c9d9b131e66b7c2bf74406100f17f7b660cff1f2f2dd9141f0f7267164"];
        let hashes: Vec<String> = MIGRATIONS
            .iter()
            .take(released.len())
            .map(|migration| blake3::hash(migration.as_bytes()).to_hex().to_string())
            .collect();
        assert_eq!(hashes, released);
    }

    #[test]
    fn a_store_made_before_migrations_counts_as_the_first() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(FIRST_SCHEMA).unwrap();
        db.pragma_update(None, "user_version", fingerprint_before_migrations())
            .unwrap();
        assert!(
            fingerprint_before_migrations() > LARGEST_COUNT,
            "can't be taken for a count"
        );

        create_or_migrate(&mut db).unwrap();

        assert_eq!(version(&db), i32::try_from(MIGRATIONS.len()).unwrap());
    }

    /// A store from v0.1.0 gains the tables added since, keeping its data.
    #[test]
    fn a_store_from_the_first_release_gains_the_workspace_look_tables() {
        let mut db = Connection::open_in_memory().unwrap();
        migrate(&mut db, &[FIRST_SCHEMA]).unwrap();

        create_or_migrate(&mut db).unwrap();

        db.execute(
            "INSERT INTO workspace_look (id, look, lamport) VALUES (1, x'00', 1)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO workspace_icon (id, blob, lamport) VALUES (1, NULL, 1)",
            [],
        )
        .unwrap();
    }

    #[test]
    fn a_store_from_before_page_mentions_gains_their_table() {
        let mut db = Connection::open_in_memory().unwrap();
        migrate(&mut db, &[FIRST_SCHEMA, WORKSPACE_LOOK]).unwrap();

        create_or_migrate(&mut db).unwrap();

        db.execute(
            "INSERT INTO page_mentions (page, author, position, preview, mentioned_ms, is_read) \
             VALUES ('p', x'00', x'00', '', 1, 0)",
            [],
        )
        .unwrap();
    }

    #[test]
    fn a_store_from_before_devices_gains_their_table_and_columns() {
        let mut db = Connection::open_in_memory().unwrap();
        migrate(&mut db, &[FIRST_SCHEMA, WORKSPACE_LOOK, PAGE_MENTIONS]).unwrap();
        db.execute(
            "INSERT INTO frozen_authors (member, last_seq) VALUES (x'01', 4)",
            [],
        )
        .unwrap();

        create_or_migrate(&mut db).unwrap();

        db.execute(
            "INSERT INTO devices (device, person, voucher, certificate, added_epoch, from_grant, added_ms) \
             VALUES (x'02', x'03', x'03', x'00', 5, 0, 1)",
            [],
        )
        .unwrap();
        let frozen_epoch: Option<u64> = db
            .query_row(
                "SELECT frozen_epoch FROM frozen_authors WHERE member = x'01'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(frozen_epoch, None, "older freezes keep no epoch");
        db.execute("UPDATE message_edits SET device = x'02'", []).unwrap();
        db.execute("UPDATE file_names SET device = x'02'", []).unwrap();
    }

    #[test]
    fn a_store_with_tables_but_no_version_is_refused_clearly() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value BLOB NOT NULL);")
            .unwrap();

        let error = create_or_migrate(&mut db).unwrap_err();

        assert!(
            error.to_string().contains("isn't a shyspace workspace"),
            "{error}"
        );
    }
}
