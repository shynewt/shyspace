//! One member's own messages across channels, newest first, for Sent.

use anyhow::Result;
use iroh::PublicKey;

use super::{
    Store,
    queries::{MESSAGE_COLUMNS, MESSAGE_JOINS, match_expression, message_from_row},
};
use crate::views::{Message, MessageCursor};

impl Store {
    /// Up to `limit` messages written by `me` in `channels`, newest first,
    /// thread replies included. With `before`, only messages older than that one;
    /// with `text`, only those matching it (each word as a prefix).
    pub fn my_messages(
        &self,
        me: &PublicKey,
        channels: &[String],
        text: &str,
        before: Option<&MessageCursor>,
        limit: u32,
    ) -> Result<Vec<Message>> {
        if channels.is_empty() {
            return Ok(Vec::new());
        }
        // One JSON list rather than a variable per channel: a member can make
        // more channels than SQLite takes variables in one statement.
        let mut conditions = vec![
            "m.author = ?".to_owned(),
            "m.channel IN (SELECT value FROM json_each(?))".to_owned(),
        ];
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(me.as_bytes().to_vec()),
            Box::new(serde_json::to_string(channels)?),
        ];
        if let Some(cursor) = before {
            conditions.push("(m.lamport < ? OR (m.lamport = ? AND m.author < ?))".into());
            values.push(Box::new(cursor.lamport));
            values.push(Box::new(cursor.lamport));
            values.push(Box::new(hex::decode(&cursor.author)?));
        }
        let match_expression = match_expression(text);
        if !match_expression.is_empty() {
            conditions
                .push("m.id IN (SELECT ref FROM search WHERE search MATCH ? AND kind = 'message')".into());
            values.push(Box::new(match_expression));
        }
        values.push(Box::new(limit));
        let sql = format!(
            "SELECT {MESSAGE_COLUMNS} {MESSAGE_JOINS}
             WHERE {}
             ORDER BY m.lamport DESC, m.author DESC LIMIT ?",
            conditions.join(" AND ")
        );
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(values.iter()), message_from_row)?;
        self.with_reactions(rows.collect::<Result<_, _>>()?, me)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::TEST_DB_KEY;

    #[test]
    fn my_messages_works_across_more_channels_than_sqlite_takes_variables() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();
        let me = iroh::SecretKey::from_bytes(&[3; 32]).public();
        store
            .db
            .execute(
                "INSERT INTO messages (id, channel, author, text, lamport, created_ms) VALUES ('m', 'mine', ?, 'hello', 1, 1)",
                [me.as_bytes().to_vec()],
            )
            .unwrap();
        // SQLite allows 32,766 variables in one statement.
        let mut channels: Vec<String> = (0..40_000).map(|index| format!("channel-{index}")).collect();
        channels.push("mine".into());

        let found = store.my_messages(&me, &channels, "", None, 50).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "hello");
    }
}
