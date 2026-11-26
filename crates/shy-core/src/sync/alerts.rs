//! Deciding which arriving messages alert this member: new ones from someone
//! else that the channel's level allows (mentions also covers replies in
//! threads you follow). Catching up on old messages (after a day offline,
//! say) never alerts.

use anyhow::Result;

use crate::{
    markdown::preview_text,
    mentions::mentions,
    ops::{MAX_MESSAGE_CHARS, Op, VerifiedOp, cut_to, now_ms},
    storage::StoredFile,
    views::{Change, NotifyLevel},
    workspace::{State, Workspace, follows_by_default},
};

pub(super) const ALERT_WINDOW_MS: i64 = 10 * 60 * 1000;
pub(crate) const PREVIEW_CHARS: usize = 140;

impl Workspace {
    /// Call only after `verified` was applied.
    pub(super) fn alert_for(&self, state: &State, verified: &VerifiedOp) -> Result<Option<Change>> {
        let Op::Message {
            id, channel, text, ..
        } = &verified.body.op
        else {
            return Ok(None);
        };
        // The author's time can be anything; saturate rather than overflow.
        if verified.author == self.me()
            || now_ms()?.saturating_sub(verified.body.timestamp_ms) > ALERT_WINDOW_MS
        {
            return Ok(None);
        }
        // Only what was stored counts, and the scans below stay bounded.
        let text = cut_to(text, MAX_MESSAGE_CHARS);
        // Your status can quiet things further, never louder than the channel.
        let my_status = state.store.member_status(self.me().as_bytes(), now_ms()?)?;
        let status_level = my_status.map_or(NotifyLevel::All, |status| status.alerts);
        let my_id = self.my_id();
        let is_wanted = match state.store.notify_level(channel)?.stricter(status_level) {
            NotifyLevel::All => true,
            NotifyLevel::Mentions => {
                mentions(text, &self.inner.config.workspace_id, &my_id)
                    || self.follows_thread_of(state, id)?
            }
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
        let names = state
            .store
            .link_names(&self.inner.config.admin, &self.inner.config.workspace_id)?;
        let text = preview_text(text, &names);
        // Files go out before their message, so they are here already.
        let files = state.store.files_of_message(id, channel)?;
        let preview: String = match (files.as_slice(), text.is_empty()) {
            ([], _) => text,
            ([file], true) => format!("{} {}", icon_of(&files), file.name),
            (_, true) => format!("{} {} files", icon_of(&files), files.len()),
            (_, false) => format!("{} {text}", icon_of(&files)),
        }
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

    /// Whether `message` is a reply in a thread this member follows.
    fn follows_thread_of(&self, state: &State, message: &str) -> Result<bool> {
        let Some(root) = state.store.thread_root(message)?.filter(|root| root != message) else {
            return Ok(false);
        };
        if let Some(choice) = state.store.following_choice(&root)? {
            return Ok(choice);
        }
        let thread = state.store.thread(&root, &self.me())?;
        let Some((first, replies)) = thread.split_first() else {
            return Ok(false);
        };
        // The new reply itself doesn't make you follow.
        let others = replies.iter().filter(|reply| reply.id != message);
        Ok(follows_by_default(
            &self.inner.config.workspace_id,
            &self.my_id(),
            (&first.author, &first.text),
            others.map(|reply| (reply.author.as_str(), reply.text.as_str())),
        ))
    }
}

/// The icon a notification shows for a message's files, as chat apps do:
/// their kind when they share one, a paperclip otherwise. The kind comes
/// from the sender's media type, which only picks the icon.
fn icon_of(files: &[StoredFile]) -> &'static str {
    let icon = |file: &StoredFile| match file.media_type.split_once('/').map(|(kind, _)| kind) {
        Some("image") => "📷",
        Some("video") => "🎥",
        Some("audio") => "🎵",
        _ => "📎",
    };
    let mut icons = files.iter().map(icon);
    let first = icons.next().unwrap_or("📎");
    if icons.all(|other| other == first) {
        first
    } else {
        "📎"
    }
}
