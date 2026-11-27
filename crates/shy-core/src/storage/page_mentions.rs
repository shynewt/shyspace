//! Unread mentions of you in pages, for the inbox: one per page and
//! author, holding where the latest is and its line as it was written.
//! Personal state, kept on this device only, so rebuilding the views keeps
//! it. A mention read is forgotten, and the newest ones are kept up to a
//! bound per author and in all, since an update may name any page id.

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use super::Store;

/// One member can mention you in this many pages before their oldest go,
/// so they can't push others' mentions out.
const MAX_PER_AUTHOR: u32 = 50;
const MAX_TOTAL: u32 = 2_000;

pub struct PageMentionRow {
    pub page: String,
    /// The author's member id (hex).
    pub author: String,
    /// An encoded Yjs relative position.
    pub position: Vec<u8>,
    pub preview: String,
    pub mentioned_ms: i64,
}

/// Where an unread mention of you is.
pub struct PageMentionSpot {
    pub page: String,
    pub author: Vec<u8>,
    pub position: Vec<u8>,
}

impl Store {
    /// Records that `author` (a person, whichever of their devices wrote)
    /// mentioned you in `page` at `position`, replacing any earlier mention
    /// of you there by them, read or not. Returns whether it is newly unread.
    pub fn note_page_mention(
        &self,
        page: &str,
        author: &[u8],
        position: &[u8],
        preview: &str,
        mentioned_ms: i64,
    ) -> Result<bool> {
        let was_unread: Option<bool> = self
            .db
            .query_row(
                "SELECT is_read = 0 FROM page_mentions WHERE page = ? AND author = ?",
                params![page, author],
                |row| row.get(0),
            )
            .optional()?;
        self.db.execute(
            "INSERT INTO page_mentions (page, author, position, preview, mentioned_ms, is_read)
             VALUES (?1, ?2, ?3, ?4, ?5, 0)
             ON CONFLICT (page, author) DO UPDATE SET position = ?3, preview = ?4, mentioned_ms = ?5, is_read = 0",
            params![page, author, position, preview, mentioned_ms],
        )?;
        self.keep_newest_page_mentions(author)?;
        let is_kept = self.exists(
            "SELECT 1 FROM page_mentions WHERE page = ? AND author = ?",
            params![page, author],
        )?;
        Ok(is_kept && was_unread != Some(true))
    }

    /// Trims `author`'s mentions, then everyone's, to their bounds, keeping
    /// unread ones first (rows read by older versions go first), then the
    /// newest. Counted first, so a mention under the bounds costs no sort.
    fn keep_newest_page_mentions(&self, author: &[u8]) -> Result<()> {
        let by_author: u32 = self.db.query_row(
            "SELECT COUNT(*) FROM page_mentions WHERE author = ?",
            [author],
            |row| row.get(0),
        )?;
        if by_author > MAX_PER_AUTHOR {
            self.db.execute(
                "DELETE FROM page_mentions WHERE author = ?1 AND rowid NOT IN
                    (SELECT rowid FROM page_mentions WHERE author = ?1
                     ORDER BY is_read, mentioned_ms DESC, rowid DESC LIMIT ?2)",
                params![author, MAX_PER_AUTHOR],
            )?;
        }
        let total: u32 = self
            .db
            .query_row("SELECT COUNT(*) FROM page_mentions", [], |row| row.get(0))?;
        if total > MAX_TOTAL {
            self.db.execute(
                "DELETE FROM page_mentions WHERE rowid NOT IN
                    (SELECT rowid FROM page_mentions ORDER BY is_read, mentioned_ms DESC, rowid DESC LIMIT ?)",
                [MAX_TOTAL],
            )?;
        }
        Ok(())
    }

    /// Unread mentions in pages that still exist, newest first.
    pub fn unread_page_mentions(&self, limit: usize) -> Result<Vec<PageMentionRow>> {
        let mut statement = self.db.prepare(
            "SELECT page, author, position, preview, mentioned_ms FROM page_mentions
             WHERE is_read = 0 AND page NOT IN (SELECT id FROM deleted)
             ORDER BY mentioned_ms DESC, author LIMIT ?",
        )?;
        let rows = statement.query_map([i64::try_from(limit)?], |row| {
            Ok(PageMentionRow {
                page: row.get(0)?,
                author: hex::encode(row.get::<_, Vec<u8>>(1)?),
                position: row.get(2)?,
                preview: row.get(3)?,
                mentioned_ms: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn unread_page_mention_count(&self) -> Result<usize> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM page_mentions WHERE is_read = 0 AND page NOT IN (SELECT id FROM deleted)",
            [],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count)?)
    }

    /// Every page with unread mentions of you, with where each is.
    pub fn page_mention_positions(&self) -> Result<Vec<PageMentionSpot>> {
        let mut statement = self
            .db
            .prepare("SELECT page, author, position FROM page_mentions WHERE is_read = 0 ORDER BY page")?;
        let rows = statement.query_map([], |row| {
            Ok(PageMentionSpot {
                page: row.get(0)?,
                author: row.get(1)?,
                position: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn forget_page_mention(&self, page: &str, author: &[u8]) -> Result<()> {
        self.db.execute(
            "DELETE FROM page_mentions WHERE page = ? AND author = ?",
            params![page, author],
        )?;
        Ok(())
    }

    /// Forgets `page`'s mentions of you. Returns whether any was unread.
    pub fn mark_page_mentions_read(&self, page: &str) -> Result<bool> {
        let unread = self
            .db
            .execute("DELETE FROM page_mentions WHERE page = ? AND is_read = 0", [page])?;
        self.db
            .execute("DELETE FROM page_mentions WHERE page = ?", [page])?;
        Ok(unread > 0)
    }

    pub fn mark_all_page_mentions_read(&self) -> Result<()> {
        self.db.execute("DELETE FROM page_mentions", [])?;
        Ok(())
    }

    /// Forgets mentions read by versions that kept them marked rather than
    /// forgotten.
    pub fn forget_read_page_mentions(&self) -> Result<()> {
        self.db
            .execute("DELETE FROM page_mentions WHERE is_read = 1", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::TEST_DB_KEY;

    fn rows(store: &Store) -> i64 {
        store
            .db
            .query_row("SELECT COUNT(*) FROM page_mentions", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn mentions_read_are_forgotten_not_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();
        assert!(store.note_page_mention("p1", b"bob", b"here", "hi", 1).unwrap());
        store.note_page_mention("p2", b"bob", b"here", "hi", 2).unwrap();

        assert!(store.mark_page_mentions_read("p1").unwrap());
        assert_eq!(rows(&store), 1);
        assert!(
            !store.mark_page_mentions_read("p1").unwrap(),
            "nothing left to read"
        );
        store.mark_all_page_mentions_read().unwrap();
        assert_eq!(rows(&store), 0);

        assert!(
            store.note_page_mention("p1", b"bob", b"again", "hi", 3).unwrap(),
            "a new one counts again"
        );
    }

    #[test]
    fn the_newest_unread_mentions_are_kept_within_the_bounds() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();
        // Read by an older version, which kept it marked.
        store
            .note_page_mention("old", b"carol", b"here", "hi", 1_000)
            .unwrap();
        store.execute_for_test("UPDATE page_mentions SET is_read = 1");
        for index in 0..MAX_TOTAL {
            let author = format!("author {}", index / MAX_PER_AUTHOR);
            store
                .note_page_mention(
                    &format!("p{index}"),
                    author.as_bytes(),
                    b"here",
                    "hi",
                    i64::from(index) + 10,
                )
                .unwrap();
        }
        assert!(
            !store
                .exists("SELECT 1 FROM page_mentions WHERE is_read = 1", [])
                .unwrap(),
            "the read one went first"
        );

        assert!(
            store
                .note_page_mention("newest", b"dave", b"here", "hi", 1_000_000)
                .unwrap()
        );
        assert!(
            !store
                .exists("SELECT 1 FROM page_mentions WHERE page = 'p0'", [])
                .unwrap(),
            "the oldest went"
        );
        assert_eq!(rows(&store), i64::from(MAX_TOTAL));

        assert!(
            !store
                .note_page_mention("ancient", b"dave", b"here", "hi", 0)
                .unwrap(),
            "not kept, so not newly unread"
        );
    }
}
