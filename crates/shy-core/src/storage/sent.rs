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
        let mut conditions = vec![
            "m.author = ?".to_owned(),
            format!("m.channel IN ({})", vec!["?"; channels.len()].join(", ")),
        ];
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(me.as_bytes().to_vec())];
        values.extend(
            channels
                .iter()
                .map(|channel| Box::new(channel.clone()) as Box<dyn rusqlite::ToSql>),
        );
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
