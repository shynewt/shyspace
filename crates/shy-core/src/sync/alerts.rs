//! Deciding which arriving messages alert this member: new ones from someone
//! else that the channel's level allows. Catching up on old messages (after
//! a day offline, say) never alerts.

use anyhow::Result;

use crate::{
    mentions::{mentions, with_plain_mentions},
    ops::{Op, VerifiedOp, now_ms},
    views::{Change, NotifyLevel},
    workspace::{State, Workspace},
};

const ALERT_WINDOW_MS: i64 = 10 * 60 * 1000;
const PREVIEW_CHARS: usize = 140;

impl Workspace {
    /// Call only after `verified` was applied.
    pub(super) fn alert_for(&self, state: &State, verified: &VerifiedOp) -> Result<Option<Change>> {
        let Op::Message {
            id, channel, text, ..
        } = &verified.body.op
        else {
            return Ok(None);
        };
        if verified.author == self.me() || now_ms()? - verified.body.timestamp_ms > ALERT_WINDOW_MS {
            return Ok(None);
        }
        let my_id = self.my_id();
        let is_wanted = match state.store.notify_level(channel)? {
            NotifyLevel::All => true,
            NotifyLevel::Mentions => mentions(text, &my_id),
            NotifyLevel::Nothing => false,
        };
        if !is_wanted {
            return Ok(None);
        }
        let author = hex::encode(verified.author.as_bytes());
        let channel_name = state
            .store
            .channels()?
            .into_iter()
            .find(|candidate| candidate.id == *channel)
            .map(|candidate| candidate.name)
            .unwrap_or_default();
        let author_name = state
            .store
            .members(&self.inner.config.admin)?
            .into_iter()
            .find(|member| member.id == author)
            .map(|member| member.name)
            .unwrap_or_default();
        let preview: String = with_plain_mentions(text)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(PREVIEW_CHARS)
            .collect();
        Ok(Some(Change::Alert {
            channel: channel.clone(),
            channel_name,
            message: id.clone(),
            author,
            author_name,
            preview,
        }))
    }
}
