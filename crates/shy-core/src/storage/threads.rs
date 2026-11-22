//! Threads. A message that replies to another lives in the thread of the
//! message it (eventually) answers, in the same channel. Roots are worked
//! out when reading, so a reply that arrives before its parent still lands
//! in the right place; one whose parent is missing shows in the channel.

use anyhow::{Context, Result};
use iroh::PublicKey;
use rusqlite::{OptionalExtension, ToSql, params};

use super::{
    Store,
    queries::{IS_SHOWN_IN_CHANNEL, MESSAGE_COLUMNS, MESSAGE_JOINS, message_from_row},
};
use crate::views::Message;

/// The deepest a reply can sit below its thread's first message and still
/// be in the thread. Every thread query stops here, so a deeper reply is in
/// no thread anywhere.
const MAX_THREAD_DEPTH: usize = 1000;

/// A reply, with the first message of its thread.
pub struct Reply {
    pub id: String,
    pub root: String,
    /// The author's member id (hex).
    pub author: String,
    pub lamport: u64,
    pub created_ms: i64,
    pub text: String,
}

impl Store {
    /// Every reply in `channel`, oldest first, with its thread's root.
    pub fn thread_replies(&self, channel: &str) -> Result<Vec<Reply>> {
        self.replies_below(channel, "m.channel = ?1", &[])
    }

    /// The replies in the threads that start at `roots`, oldest first. A
    /// root that is itself a reply in this channel has no thread.
    pub fn replies_in_threads(&self, channel: &str, roots: &[String]) -> Result<Vec<Reply>> {
        if roots.is_empty() {
            return Ok(Vec::new());
        }
        // One JSON value rather than a `?` per root: SQLite takes at most
        // 32766 values, and a thread can hold more replies than that. The
        // `+` keeps SQLite from reading the channel's index to find a few
        // messages it can look up by id.
        let roots = serde_json::to_string(roots).context("listing thread roots")?;
        self.replies_below(
            channel,
            "m.id IN (SELECT value FROM json_each(?2)) AND +m.channel = ?1",
            &[&roots],
        )
    }

    /// Replies under the messages shown in `channel` (`?1`) that match
    /// `roots_filter`, whose other values are numbered from `?2`.
    fn replies_below(&self, channel: &str, roots_filter: &str, values: &[&dyn ToSql]) -> Result<Vec<Reply>> {
        // UNION, not UNION ALL: a reply loop made on purpose still ends.
        // CROSS JOIN keeps SQLite going from the chain to messages (by
        // index), rather than scanning messages for each step.
        let mut statement = self.db.prepare(&format!(
            "WITH RECURSIVE chain(id, root, depth) AS (
                SELECT m.id, m.id, 0 FROM messages m
                WHERE {roots_filter} AND {IS_SHOWN_IN_CHANNEL}
                UNION
                SELECT m.id, chain.root, chain.depth + 1 FROM chain CROSS JOIN messages m ON m.reply_to = chain.id
                WHERE m.channel = ?1 AND chain.depth < {MAX_THREAD_DEPTH}
             )
             SELECT chain.id, chain.root, m.author, m.lamport, m.created_ms, COALESCE(e.text, m.text)
             FROM chain CROSS JOIN messages m ON m.id = chain.id
             LEFT JOIN message_edits e ON e.message = m.id
             WHERE chain.id != chain.root
             ORDER BY m.lamport, m.author, m.id"
        ))?;
        let mut parameters: Vec<&dyn ToSql> = vec![&channel];
        parameters.extend_from_slice(values);
        let rows = statement.query_map(parameters.as_slice(), |row| {
            Ok(Reply {
                id: row.get(0)?,
                root: row.get(1)?,
                author: hex::encode(row.get::<_, Vec<u8>>(2)?),
                lamport: row.get(3)?,
                created_ms: row.get(4)?,
                text: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn message(&self, id: &str, me: &PublicKey) -> Result<Option<Message>> {
        let message = self
            .db
            .query_row(
                &format!("SELECT {MESSAGE_COLUMNS} {MESSAGE_JOINS} WHERE m.id = ?"),
                [id],
                message_from_row,
            )
            .optional()?;
        Ok(match message {
            Some(message) => self.with_reactions(vec![message], me)?.pop(),
            None => None,
        })
    }

    /// The newest message in `channel`, thread replies included.
    pub fn latest_message(&self, channel: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT id FROM messages WHERE channel = ? ORDER BY lamport DESC, author DESC LIMIT 1",
                [channel],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// The thread starting at `root`: its first message, then every reply.
    pub fn thread(&self, root: &str, me: &PublicKey) -> Result<Vec<Message>> {
        let mut statement = self.db.prepare(&format!(
            "WITH RECURSIVE chain(id, depth) AS (
                SELECT ?1, 0
                UNION
                SELECT m.id, chain.depth + 1 FROM messages m JOIN chain ON m.reply_to = chain.id
                WHERE m.channel = (SELECT channel FROM messages WHERE id = ?1) AND chain.depth < {MAX_THREAD_DEPTH}
             )
             SELECT {MESSAGE_COLUMNS} {MESSAGE_JOINS}
             WHERE m.id IN (SELECT id FROM chain)
             ORDER BY m.id != ?1, m.lamport, m.author, m.id"
        ))?;
        let rows = statement.query_map([root], message_from_row)?;
        self.with_reactions(rows.collect::<Result<_, _>>()?, me)
    }

    /// The first message of the thread `message` is in (itself if it isn't
    /// a reply), or `None` if `message` doesn't exist or is in no thread: its
    /// replies loop (only a forged client makes that) or sit deeper than
    /// `MAX_THREAD_DEPTH`. Such messages show neither in the channel nor in
    /// a thread.
    pub fn thread_root(&self, message: &str) -> Result<Option<String>> {
        let mut current = message.to_owned();
        for _ in 0..=MAX_THREAD_DEPTH {
            let row: Option<(String, Option<String>)> = self
                .db
                .query_row(
                    "SELECT channel, reply_to FROM messages WHERE id = ?",
                    [&current],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((channel, reply_to)) = row else {
                return Ok(None);
            };
            let parent: Option<String> = match reply_to {
                Some(parent) => self
                    .db
                    .query_row(
                        "SELECT id FROM messages WHERE id = ? AND channel = ?",
                        [&parent, &channel],
                        |row| row.get(0),
                    )
                    .optional()?,
                None => None,
            };
            match parent {
                Some(parent) => current = parent,
                None => return Ok(Some(current)),
            }
        }
        Ok(None)
    }

    /// Your explicit choice to follow a thread or not, if you made one.
    pub fn following_choice(&self, root: &str) -> Result<Option<bool>> {
        Ok(self
            .db
            .query_row(
                "SELECT is_following FROM thread_follows WHERE root = ?",
                [root],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_following(&self, root: &str, is_following: bool) -> Result<()> {
        self.db.execute(
            "INSERT INTO thread_follows (root, is_following) VALUES (?1, ?2)
             ON CONFLICT (root) DO UPDATE SET is_following = ?2",
            params![root, is_following],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::MAX_THREAD_DEPTH;
    use crate::storage::test_channel::TestChannel;

    #[test]
    fn replies_in_threads_follow_each_chain_to_its_first_message() {
        let mut channel = TestChannel::new();
        let root = channel.post("first", None);
        let reply = channel.post("reply", Some(&root));
        let nested = channel.post("reply to the reply", Some(&reply));
        let other = channel.post_threads(1);
        let orphan = channel.post("its parent never arrived", Some("missing"));
        let orphan_reply = channel.post("reply to the orphan", Some(&orphan));

        let wanted = [root.clone(), reply.clone(), orphan.clone()];
        let replies = channel.store.replies_in_threads(&channel.id, &wanted).unwrap();
        let found: Vec<(&str, &str)> = replies
            .iter()
            .map(|reply| (reply.id.as_str(), reply.root.as_str()))
            .collect();

        assert_eq!(
            found,
            [
                (reply.as_str(), root.as_str()),
                (nested.as_str(), root.as_str()),
                (orphan_reply.as_str(), orphan.as_str())
            ]
        );
        assert!(replies.iter().all(|found| found.root != other[0]));
    }

    #[test]
    fn replies_in_threads_take_more_roots_than_sqlite_takes_values() {
        let mut channel = TestChannel::new();
        let roots = channel.post_threads(2);
        let mut asked: Vec<String> = (0..40_000).map(|number| format!("absent{number}")).collect();
        asked.extend(roots);

        let replies = channel.store.replies_in_threads(&channel.id, &asked).unwrap();

        assert_eq!(replies.len(), 4);
    }

    /// Only a forged op can tie on lamport and author; the order is still the
    /// same on every device.
    #[test]
    fn replies_that_tie_come_in_id_order() {
        let mut channel = TestChannel::new();
        let root = channel.post("first", None);
        let mut ids: Vec<String> = (0..3)
            .map(|_| crate::ops::new_owned_id(&channel.author))
            .collect();
        ids.sort();
        // Stored in reverse, so only the tie-break can put them in id order.
        for id in ids.iter().rev() {
            channel.apply_again(crate::ops::Op::Message {
                id: id.clone(),
                channel: channel.id.clone(),
                text: "tied".into(),
                reply_to: Some(root.clone()),
            });
        }

        let replies = channel.store.replies_in_threads(&channel.id, &[root]).unwrap();

        let found: Vec<String> = replies.into_iter().map(|reply| reply.id).collect();
        assert_eq!(found, ids);
    }

    /// A reply deeper than a thread follows is in no thread: not in the
    /// thread, not among its replies, and without a root, all alike.
    #[test]
    fn replies_past_the_deepest_a_thread_follows_are_in_no_thread() {
        let mut channel = TestChannel::new();
        let root = channel.post("first", None);
        let mut chain = vec![root.clone()];
        for _ in 0..=MAX_THREAD_DEPTH {
            let parent = chain.last().unwrap().clone();
            chain.push(channel.post("and then", Some(&parent)));
        }
        let deepest_kept = &chain[MAX_THREAD_DEPTH];
        let too_deep = chain.last().unwrap();
        let me = channel.author;

        let thread: Vec<String> = channel
            .store
            .thread(&root, &me)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        let replies = channel
            .store
            .replies_in_threads(&channel.id, std::slice::from_ref(&root))
            .unwrap();

        assert_eq!(thread.len(), MAX_THREAD_DEPTH + 1);
        assert!(thread.contains(deepest_kept) && !thread.contains(too_deep));
        assert_eq!(replies.len(), MAX_THREAD_DEPTH);
        assert_eq!(channel.store.thread_root(deepest_kept).unwrap(), Some(root));
        assert_eq!(channel.store.thread_root(too_deep).unwrap(), None);
    }

    #[test]
    fn listing_a_channels_threads_grows_with_the_channel_not_its_square() {
        let mut channel = TestChannel::new();
        channel.post_threads(100);
        let with_few = channel.store.steps_of(|| {
            channel.store.thread_replies(&channel.id).unwrap();
        });
        channel.post_threads(300);
        let with_four_times = channel.store.steps_of(|| {
            channel.store.thread_replies(&channel.id).unwrap();
        });

        assert!(
            with_four_times < with_few * 6,
            "{with_few} steps for 100 threads, {with_four_times} for 400"
        );
    }

    #[test]
    fn replies_for_a_page_cost_the_same_however_big_the_channel() {
        let mut channel = TestChannel::new();
        let page = channel.post_threads(5);
        channel.post_threads(20);
        let with_few = channel.store.steps_of(|| {
            channel.store.replies_in_threads(&channel.id, &page).unwrap();
        });
        channel.post_threads(500);
        let with_many = channel.store.steps_of(|| {
            channel.store.replies_in_threads(&channel.id, &page).unwrap();
        });

        assert!(
            with_many < with_few * 2,
            "{with_few} steps with 25 threads, {with_many} with 525"
        );
    }
}
