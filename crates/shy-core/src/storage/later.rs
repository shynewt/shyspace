//! Later: messages and pages saved for yourself, with optional reminders.
//! Personal state, kept on this device only.

use std::collections::HashSet;

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::views::SavedMark;

pub struct SavedRow {
    pub item: String,
    pub saved_ms: i64,
    pub remind_ms: Option<i64>,
    pub is_done: bool,
}

impl Store {
    /// Saves `item`, or updates its reminder if already saved. A new
    /// reminder time fires again even if an earlier one already did.
    pub fn save_for_later(&self, item: &str, remind_ms: Option<i64>, now_ms: i64) -> Result<()> {
        self.db.execute(
            "INSERT INTO later (item, saved_ms, remind_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT (item) DO UPDATE SET remind_ms = ?3, is_reminded = 0, is_done = 0",
            params![item, now_ms, remind_ms],
        )?;
        Ok(())
    }

    pub fn remove_from_later(&self, item: &str) -> Result<()> {
        self.db.execute("DELETE FROM later WHERE item = ?", [item])?;
        Ok(())
    }

    pub fn mark_later_done(&self, item: &str, is_done: bool) -> Result<()> {
        self.db.execute(
            "UPDATE later SET is_done = ?2 WHERE item = ?1",
            params![item, is_done],
        )?;
        Ok(())
    }

    /// Everything saved, newest first.
    pub fn later(&self) -> Result<Vec<SavedRow>> {
        let mut statement = self.db.prepare(
            "SELECT item, saved_ms, remind_ms, is_done FROM later ORDER BY saved_ms DESC, rowid DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(SavedRow {
                item: row.get(0)?,
                saved_ms: row.get(1)?,
                remind_ms: row.get(2)?,
                is_done: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn saved_items(&self) -> Result<HashSet<String>> {
        let mut statement = self.db.prepare("SELECT item FROM later")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Items whose reminder is due and hasn't fired yet, marked as fired.
    pub fn take_due_reminders(&self, now_ms: i64) -> Result<Vec<String>> {
        let mut statement = self.db.prepare(
            "SELECT item FROM later
             WHERE remind_ms IS NOT NULL AND remind_ms <= ? AND is_reminded = 0 AND is_done = 0
             ORDER BY remind_ms",
        )?;
        let due: Vec<String> = statement
            .query_map([now_ms], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        for item in &due {
            self.db
                .execute("UPDATE later SET is_reminded = 1 WHERE item = ?", [item])?;
        }
        Ok(due)
    }

    pub fn saved_mark(&self, item: &str) -> Result<Option<SavedMark>> {
        Ok(self
            .db
            .query_row(
                "SELECT remind_ms, is_done FROM later WHERE item = ?",
                [item],
                |row| {
                    Ok(SavedMark {
                        remind_ms: row.get(0)?,
                        is_done: row.get(1)?,
                    })
                },
            )
            .optional()?)
    }
}
