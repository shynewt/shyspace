//! Page updates merged in memory during a transaction and written once, when
//! it commits: a batch of keystrokes then costs the size of each update
//! rather than the whole page each time. A savepoint that rolls back takes
//! back what it merged, so memory always matches what the database will keep.

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use rusqlite::params;

use super::Store;
use crate::{
    ops::rule_violation,
    pages::{MergeLost, PageMerge},
};

#[derive(Default)]
pub(super) struct PendingPages {
    pages: BTreeMap<String, PendingPage>,
    /// What happened to `pages`, so a rolled-back savepoint can be undone.
    journal: Vec<Step>,
    /// Where each open savepoint's steps begin in `journal`.
    savepoints: Vec<usize>,
    /// A savepoint couldn't be ended or undone, so memory may no longer
    /// match the database: the transaction must not commit.
    has_lost_track: bool,
}

struct PendingPage {
    merge: PageMerge,
    updated_ms: i64,
}

enum Step {
    /// The page was loaded from the database.
    Loaded(String),
    /// An update was merged into the page; holds its time before.
    Merged { page: String, updated_ms: i64 },
    /// A rebuild replaced every page from the log; holds what was pending.
    Replaced(BTreeMap<String, PendingPage>),
}

impl PendingPages {
    pub(super) fn begin_savepoint(&mut self) {
        self.savepoints.push(self.journal.len());
    }

    /// The savepoint's steps now belong to the one around it.
    pub(super) fn release_savepoint(&mut self) {
        self.savepoints.pop();
    }

    pub(super) fn roll_back_savepoint(&mut self) -> Result<()> {
        let start = self
            .savepoints
            .pop()
            .context("rolling back a savepoint that was never begun")?;
        while self.journal.len() > start {
            match self.journal.pop().context("the journal shrank")? {
                Step::Loaded(page) => {
                    self.pages.remove(&page);
                }
                Step::Merged { page, updated_ms } => {
                    let pending = self
                        .pages
                        .get_mut(&page)
                        .context("a merged page left the cache")?;
                    pending.merge.undo_last()?;
                    pending.updated_ms = updated_ms;
                }
                Step::Replaced(pages) => self.pages = pages,
            }
        }
        Ok(())
    }

    pub(super) fn lose_track(&mut self) {
        self.has_lost_track = true;
    }

    /// Forgets everything, as when the transaction ends.
    fn take(&mut self) -> BTreeMap<String, PendingPage> {
        std::mem::take(self).pages
    }
}

impl Store {
    /// Merges `update` into `page`, written as Yjs client `client`, until
    /// the transaction commits. Refuses what [`crate::pages::merge`] would,
    /// as a broken rule.
    pub(super) fn merge_page_update(&self, page: &str, update: &[u8], client: u64, now: i64) -> Result<()> {
        ensure!(
            self.is_in_transaction(),
            "page updates are merged inside a transaction"
        );
        if !self.pending_pages.borrow().pages.contains_key(page) {
            let state = self.stored_page_state(page)?;
            let merge = PageMerge::from_state(state.as_deref()).map_err(damaged)?;
            let mut pending = self.pending_pages.borrow_mut();
            pending.pages.insert(
                page.to_owned(),
                PendingPage {
                    merge,
                    updated_ms: i64::MIN,
                },
            );
            pending.journal.push(Step::Loaded(page.to_owned()));
        }
        let mut pending = self.pending_pages.borrow_mut();
        let entry = pending
            .pages
            .get_mut(page)
            .context("a loaded page left the cache")?;
        if let Err(error) = entry.merge.add(update, client) {
            if error.downcast_ref::<MergeLost>().is_some() {
                pending.lose_track();
                return Err(error);
            }
            return Err(damaged(error));
        }
        let updated_ms = entry.updated_ms;
        entry.updated_ms = updated_ms.max(now);
        pending.journal.push(Step::Merged {
            page: page.to_owned(),
            updated_ms,
        });
        Ok(())
    }

    /// A page's state including what's merged but not yet written.
    pub(super) fn pending_page_state(&self, page: &str) -> Option<Vec<u8>> {
        let pending = self.pending_pages.borrow();
        let entry = pending.pages.get(page)?;
        (!entry.merge.is_empty()).then(|| entry.merge.state())
    }

    /// A rebuild writes every page from the log, which already holds what
    /// is pending; a rollback brings the pending pages back.
    pub(super) fn replace_pending_pages(&self) {
        let mut pending = self.pending_pages.borrow_mut();
        let pages = std::mem::take(&mut pending.pages);
        pending.journal.push(Step::Replaced(pages));
    }

    pub(super) fn forget_pending_pages(&self) {
        self.pending_pages.borrow_mut().take();
    }

    /// Writes each page merged in this transaction once, with its search entry.
    pub(super) fn write_pending_pages(&self) -> Result<()> {
        let pending = std::mem::take(&mut *self.pending_pages.borrow_mut());
        ensure!(
            !pending.has_lost_track,
            "lost track of the page updates in this transaction; it is applied again"
        );
        let pages = pending.pages;
        for (page, pending) in pages {
            if pending.merge.is_empty() || self.is_deleted(&page)? {
                continue;
            }
            self.db.execute(
                "UPDATE pages SET doc = ?, updated_ms = MAX(updated_ms, ?) WHERE id = ?",
                params![pending.merge.state(), pending.updated_ms, page],
            )?;
            self.reindex_page(&page)?;
        }
        Ok(())
    }
}

fn damaged(error: anyhow::Error) -> anyhow::Error {
    rule_violation(format!("damaged page update: {error:#}"))
}

#[cfg(test)]
mod tests {
    use anyhow::bail;

    use super::*;
    use crate::{
        pages::{self, Edit},
        storage::TEST_DB_KEY,
    };

    const PAGE: &str = "plan";
    const ADA: u64 = 1;
    const BOB: u64 = 2;

    fn store_with_page(dir: &tempfile::TempDir) -> Store {
        let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();
        store.execute_for_test("INSERT INTO pages (id, title) VALUES ('plan', 'Plan')");
        store
    }

    /// Ada's "one", then Bob's "two" written on top of it.
    fn two_updates() -> (Vec<u8>, Vec<u8>) {
        let first = pages::edit(None, Edit::Append("one"), ADA).unwrap();
        let state = pages::merge(None, &first, ADA).unwrap();
        let second = pages::edit(Some(&state), Edit::Append("two"), BOB).unwrap();
        (first, second)
    }

    fn text(store: &Store) -> String {
        pages::read(store.page_state(PAGE).unwrap().as_deref()).unwrap()
    }

    #[test]
    fn a_rolled_back_savepoint_takes_back_its_page_updates() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_page(&dir);
        let (first, second) = two_updates();
        store
            .atomically(|store| {
                store.merge_page_update(PAGE, &first, ADA, 1)?;
                let later = store.with_savepoint(|store| -> Result<()> {
                    store.merge_page_update(PAGE, &second, BOB, 2)?;
                    bail!("a later step of the same op fails")
                });
                assert!(later.is_err());
                Ok(())
            })
            .unwrap();
        assert_eq!(text(&store), "one");
    }

    #[test]
    fn a_rolled_back_rebuild_brings_the_pending_pages_back() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_page(&dir);
        let (first, _) = two_updates();
        store
            .atomically(|store| {
                store.merge_page_update(PAGE, &first, ADA, 1)?;
                let rebuild = store.with_savepoint(|store| -> Result<()> {
                    store.replace_pending_pages();
                    bail!("the rebuild fails")
                });
                assert!(rebuild.is_err());
                Ok(())
            })
            .unwrap();
        assert_eq!(text(&store), "one");
    }

    #[test]
    fn a_failed_transaction_writes_nothing_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_page(&dir);
        let (first, second) = two_updates();
        let failed = store.atomically(|store| -> Result<()> {
            store.merge_page_update(PAGE, &first, ADA, 1)?;
            bail!("the transaction fails")
        });
        assert!(failed.is_err());
        assert_eq!(store.page_state(PAGE).unwrap(), None);

        // Bob's update needs Ada's, which never landed.
        store
            .atomically(|store| store.merge_page_update(PAGE, &first, ADA, 1))
            .unwrap();
        store
            .atomically(|store| store.merge_page_update(PAGE, &second, BOB, 2))
            .unwrap();
        assert_eq!(text(&store), "one\n\ntwo");
    }

    #[test]
    fn a_transaction_that_lost_track_of_its_pages_commits_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_page(&dir);
        let (first, _) = two_updates();
        let committed = store.atomically(|store| {
            store.merge_page_update(PAGE, &first, ADA, 1)?;
            store.pending_pages.borrow_mut().lose_track();
            Ok(())
        });
        assert!(committed.is_err());
        assert_eq!(store.page_state(PAGE).unwrap(), None);
    }

    #[test]
    fn updates_merged_in_one_transaction_write_the_page_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_page(&dir);
        let (first, second) = two_updates();
        let writes = store.count_page_writes();
        store
            .atomically(|store| {
                store.merge_page_update(PAGE, &first, ADA, 1)?;
                assert_eq!(text(store), "one", "reads see what's pending");
                store.merge_page_update(PAGE, &second, BOB, 2)
            })
            .unwrap();
        assert_eq!(writes.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(text(&store), "one\n\ntwo");
    }
}
