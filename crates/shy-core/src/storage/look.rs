//! The workspace's default look, which the admin sets.

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::views::WorkspaceLook;

type EncodedLook<'a> = (Option<&'a str>, Option<&'a str>);

impl Store {
    /// Keeps `look` unless a newer one is kept. A tie, which only a forged
    /// op makes, goes to the larger encoded look, so every order ends the
    /// same; the same look arriving again changes nothing. Returns whether
    /// the look shown changed.
    pub(super) fn put_workspace_look(&self, look: &WorkspaceLook, lamport: u64) -> Result<bool> {
        let encoded: EncodedLook<'_> = (look.theme.as_deref(), look.accent.as_deref());
        let encoded = postcard::to_stdvec(&encoded)?;
        let kept: Option<(Vec<u8>, u64)> = self
            .db
            .query_row(
                "SELECT look, lamport FROM workspace_look WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if kept
            .is_some_and(|(kept_look, kept_lamport)| (kept_lamport, kept_look) >= (lamport, encoded.clone()))
        {
            return Ok(false);
        }
        self.db.execute(
            "INSERT INTO workspace_look (id, look, lamport) VALUES (1, ?1, ?2)
             ON CONFLICT (id) DO UPDATE SET look = ?1, lamport = ?2",
            params![encoded, lamport],
        )?;
        Ok(true)
    }

    /// The look the admin set; the app's default where none is set.
    pub fn workspace_look(&self) -> Result<WorkspaceLook> {
        let kept: Option<Vec<u8>> = self
            .db
            .query_row("SELECT look FROM workspace_look WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()?;
        let Some(bytes) = kept else {
            return Ok(WorkspaceLook::default());
        };
        let (theme, accent): (Option<String>, Option<String>) =
            postcard::from_bytes(&bytes).context("reading the stored workspace look")?;
        Ok(WorkspaceLook { theme, accent })
    }
}
