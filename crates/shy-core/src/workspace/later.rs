//! Later: messages and pages you saved for yourself, with reminders.

use anyhow::{Result, bail};

use super::Workspace;
use crate::{
    ops::now_ms,
    storage::ItemKind,
    views::{Change, LaterItem},
};

impl Workspace {
    /// Saves a message or page, optionally with a reminder (ms since the
    /// epoch). Saving again changes the reminder.
    pub fn save_for_later(&self, item: &str, remind_ms: Option<i64>) -> Result<()> {
        let kind = self.state().store.item_kind(item)?;
        if !matches!(kind, Some(ItemKind::Message | ItemKind::Page)) {
            bail!("only messages and pages can be saved for later");
        }
        self.state().store.save_for_later(item, remind_ms, now_ms()?)?;
        self.emit_later_change(item)
    }

    pub fn remove_from_later(&self, item: &str) -> Result<()> {
        self.state().store.remove_from_later(item)?;
        self.emit_later_change(item)
    }

    pub fn mark_later_done(&self, item: &str, is_done: bool) -> Result<()> {
        self.state().store.mark_later_done(item, is_done)?;
        self.emit_later_change(item)
    }

    /// Everything saved, newest first. Items since deleted are left out.
    pub fn later(&self) -> Result<Vec<LaterItem>> {
        let rows = self.state().store.later()?;
        let pages = self.pages()?;
        let mut items = Vec::new();
        for row in rows {
            let message = self.linked_message(&row.item)?;
            let page = pages.iter().find(|page| page.id == row.item).cloned();
            if message.is_none() && page.is_none() {
                continue;
            }
            items.push(LaterItem {
                id: row.item,
                saved_ms: row.saved_ms,
                remind_ms: row.remind_ms,
                is_done: row.is_done,
                message,
                page,
            });
        }
        Ok(items)
    }

    /// Saved items whose reminder came due, each handed out once. Reminders
    /// due while the app was closed come out at the next call.
    pub fn take_due_reminders(&self) -> Result<Vec<LaterItem>> {
        let due = self.state().store.take_due_reminders(now_ms()?)?;
        if due.is_empty() {
            return Ok(Vec::new());
        }
        let items = self
            .later()?
            .into_iter()
            .filter(|item| due.contains(&item.id))
            .collect();
        self.emit(Change::Later);
        Ok(items)
    }

    /// Later changed; a saved message's view changed too (its saved mark).
    fn emit_later_change(&self, item: &str) -> Result<()> {
        self.emit(Change::Later);
        if let Some(linked) = self.linked_message(item)? {
            self.emit(Change::Messages {
                channel: linked.message.channel,
            });
        } else {
            self.emit(Change::Pages);
        }
        Ok(())
    }
}
