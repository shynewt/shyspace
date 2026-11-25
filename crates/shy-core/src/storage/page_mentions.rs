//! Unread mentions of you in pages, for the inbox: one per page and
//! author, holding where the latest is and its line as it was written.
//! Personal state, kept on this device only, so rebuilding the views keeps
//! it.

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use super::Store;

pub struct PageMentionRow {
    pub page: String,
    /// The author's member id (hex).
    pub author: String,
    /// An encoded Yjs relative position.
    pub position: Vec<u8>,
    pub preview: String,
    pub mentioned_ms: i64,
}

/// Where a mention of you is, read or not.
pub struct PageMentionSpot {
    pub page: String,
    pub author: Vec<u8>,
    pub position: Vec<u8>,
}

impl Store {
    /// Records that `author` mentioned you in `page` at `position`,
    /// replacing any earlier mention of you there by them, read or not.
    /// Returns whether it is newly unread.
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
        Ok(was_unread != Some(true))
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

    /// Every page with mentions of you, with where each is, read or not.
    pub fn page_mention_positions(&self) -> Result<Vec<PageMentionSpot>> {
        let mut statement = self
            .db
            .prepare("SELECT page, author, position FROM page_mentions ORDER BY page")?;
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

    /// Returns whether any was unread.
    pub fn mark_page_mentions_read(&self, page: &str) -> Result<bool> {
        let changed = self.db.execute(
            "UPDATE page_mentions SET is_read = 1 WHERE page = ? AND is_read = 0",
            [page],
        )?;
        Ok(changed > 0)
    }

    pub fn mark_all_page_mentions_read(&self) -> Result<()> {
        self.db
            .execute("UPDATE page_mentions SET is_read = 1 WHERE is_read = 0", [])?;
        Ok(())
    }
}
