//! Sidebar sections: personal groups of channels, kept on this device only.

use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::views::SectionSort;

/// The section every channel is in until you move it.
pub const BUILTIN_SECTION: &str = "channels";

pub struct SectionRow {
    pub id: String,
    pub name: String,
    pub sort: SectionSort,
    pub is_collapsed: bool,
}

impl Store {
    pub fn sections(&self) -> Result<Vec<SectionRow>> {
        let mut statement = self
            .db
            .prepare("SELECT id, name, sort, is_collapsed FROM sidebar_sections ORDER BY position, rowid")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get(1)?,
                row.get::<_, String>(2)?,
                row.get(3)?,
            ))
        })?;
        let mut sections = Vec::new();
        for row in rows {
            let (id, name, sort, is_collapsed) = row?;
            let Some(sort) = SectionSort::parse(&sort) else {
                bail!("unknown section sort {sort:?}");
            };
            sections.push(SectionRow {
                id,
                name,
                sort,
                is_collapsed,
            });
        }
        Ok(sections)
    }

    pub fn create_section(&self, id: &str, name: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO sidebar_sections (id, name, position)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(position), 0) + 1 FROM sidebar_sections))",
            params![id, name],
        )?;
        Ok(())
    }

    pub fn rename_section(&self, id: &str, name: &str) -> Result<()> {
        self.require_custom_section(id)?;
        self.db.execute(
            "UPDATE sidebar_sections SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        Ok(())
    }

    /// Its channels go back to the built-in section.
    pub fn delete_section(&self, id: &str) -> Result<()> {
        self.require_custom_section(id)?;
        self.db
            .execute("DELETE FROM sidebar_placement WHERE section = ?", [id])?;
        self.db
            .execute("DELETE FROM sidebar_sections WHERE id = ?", [id])?;
        Ok(())
    }

    pub fn set_section_sort(&self, id: &str, sort: SectionSort) -> Result<()> {
        self.require_section(id)?;
        self.db.execute(
            "UPDATE sidebar_sections SET sort = ?2 WHERE id = ?1",
            params![id, sort.as_str()],
        )?;
        Ok(())
    }

    pub fn set_section_collapsed(&self, id: &str, is_collapsed: bool) -> Result<()> {
        self.require_section(id)?;
        self.db.execute(
            "UPDATE sidebar_sections SET is_collapsed = ?2 WHERE id = ?1",
            params![id, is_collapsed],
        )?;
        Ok(())
    }

    /// Puts `channel` in `section`, or back in the built-in one.
    pub fn place_channel(&self, channel: &str, section: Option<&str>) -> Result<()> {
        match section {
            Some(section) if section != BUILTIN_SECTION => {
                self.require_section(section)?;
                self.db.execute(
                    "INSERT INTO sidebar_placement (channel, section) VALUES (?1, ?2)
                     ON CONFLICT (channel) DO UPDATE SET section = ?2",
                    params![channel, section],
                )?;
            }
            _ => {
                self.db
                    .execute("DELETE FROM sidebar_placement WHERE channel = ?", [channel])?;
            }
        }
        Ok(())
    }

    pub fn placement(&self, channel: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT section FROM sidebar_placement WHERE channel = ?",
                [channel],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// When the newest message in `channel` was written, if any.
    pub fn last_activity_ms(&self, channel: &str) -> Result<Option<i64>> {
        Ok(self.db.query_row(
            "SELECT MAX(created_ms) FROM messages WHERE channel = ?",
            [channel],
            |row| row.get(0),
        )?)
    }

    fn require_section(&self, id: &str) -> Result<()> {
        let exists = self
            .db
            .query_row("SELECT 1 FROM sidebar_sections WHERE id = ?", [id], |_| Ok(()))
            .optional()?
            .is_some();
        if !exists {
            bail!("no such section");
        }
        Ok(())
    }

    fn require_custom_section(&self, id: &str) -> Result<()> {
        if id == BUILTIN_SECTION {
            bail!("the Channels section can't be renamed or deleted");
        }
        self.require_section(id)
    }
}
