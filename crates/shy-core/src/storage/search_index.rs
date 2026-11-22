//! The full-text index: one entry per message or page.

use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, params};

use super::Store;

impl Store {
    /// Replaces the entry for `kind` `id` with one for this text.
    pub(super) fn index_for_search(&self, kind: &str, id: &str, title: &str, body: &str) -> Result<()> {
        self.remove_from_search(kind, id)?;
        self.db.execute(
            "INSERT INTO search (kind, ref, title, body) VALUES (?, ?, ?, ?)",
            params![kind, id, title, body],
        )?;
        self.db.execute(
            "INSERT INTO search_rows (kind, ref, search_rowid) VALUES (?, ?, last_insert_rowid())",
            params![kind, id],
        )?;
        Ok(())
    }

    pub(super) fn remove_from_search(&self, kind: &str, id: &str) -> Result<()> {
        let search_rowid: Option<i64> = self
            .db
            .query_row(
                "DELETE FROM search_rows WHERE kind = ? AND ref = ? RETURNING search_rowid",
                params![kind, id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(search_rowid) = search_rowid else {
            return Ok(());
        };
        let removed = self.db.execute(
            "DELETE FROM search WHERE rowid = ? AND kind = ? AND ref = ?",
            params![search_rowid, kind, id],
        )?;
        ensure!(removed == 1, "the search entry for {kind} {id} is missing");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{SearchFilters, TEST_DB_KEY};

    fn open_store(dir: &tempfile::TempDir) -> Store {
        Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap()
    }

    fn found(store: &Store, text: &str) -> Vec<(String, String)> {
        store
            .search(text, &SearchFilters::default(), 50)
            .unwrap()
            .into_iter()
            .map(|hit| (hit.kind, hit.id))
            .collect()
    }

    #[test]
    fn reindexing_and_removing_keep_one_entry_per_item() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        store
            .index_for_search("message", "m1", "", "first draft")
            .unwrap();
        store
            .index_for_search("page", "m1", "Plan", "first steps")
            .unwrap();
        store
            .index_for_search("message", "m1", "", "final words")
            .unwrap();

        assert!(found(&store, "draft").is_empty());
        assert_eq!(
            found(&store, "final"),
            [("message".to_string(), "m1".to_string())]
        );
        assert_eq!(found(&store, "first"), [("page".to_string(), "m1".to_string())]);

        store.remove_from_search("message", "m1").unwrap();
        assert!(found(&store, "final").is_empty());
        assert_eq!(found(&store, "first").len(), 1, "the page with the same id stays");
    }

    #[test]
    fn updating_an_entry_costs_the_same_however_much_is_indexed() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(&dir);
        let index_messages = |range: std::ops::Range<u32>| {
            for number in range {
                let id = format!("m{number}");
                store
                    .index_for_search("message", &id, "", "words to find")
                    .unwrap();
            }
        };
        index_messages(0..20);
        let with_few = store.steps_of(|| {
            store.index_for_search("message", "m5", "", "changed").unwrap();
        });
        index_messages(20..5_000);
        let with_many = store.steps_of(|| {
            store.index_for_search("message", "m6", "", "changed").unwrap();
        });

        assert!(
            with_many < with_few * 2,
            "{with_few} steps with 20 indexed, {with_many} with 5,000"
        );
    }
}
