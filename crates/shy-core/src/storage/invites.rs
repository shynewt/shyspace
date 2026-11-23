//! The admin's invites, the requests that arrive through them, and answers
//! still to deliver.

use anyhow::{Result, bail};
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::{Store, to_public_key};
use crate::views::{Invite, InviteState, member_id, safety_code};

fn state_name(state: InviteState) -> &'static str {
    match state {
        InviteState::Open => "open",
        InviteState::Requested => "requested",
        InviteState::Accepted => "accepted",
        InviteState::Rejected => "rejected",
        InviteState::Revoked => "revoked",
        InviteState::Expired => "expired",
    }
}

fn parse_state(name: &str) -> Result<InviteState> {
    Ok(match name {
        "open" => InviteState::Open,
        "requested" => InviteState::Requested,
        "accepted" => InviteState::Accepted,
        "rejected" => InviteState::Rejected,
        "revoked" => InviteState::Revoked,
        "expired" => InviteState::Expired,
        other => bail!("unknown invite state {other:?} in the database"),
    })
}

/// A join request the admin has received and not yet decided on.
pub struct PendingRequest {
    pub joiner: PublicKey,
    pub request: Vec<u8>,
}

impl Store {
    pub fn insert_invite(&self, token: &str, label: &str, created_ms: i64, expires_ms: i64) -> Result<()> {
        self.db.execute(
            "INSERT INTO invites (token, label, created_ms, expires_ms, state) VALUES (?, ?, ?, ?, ?)",
            params![
                token,
                label,
                created_ms,
                expires_ms,
                state_name(InviteState::Open)
            ],
        )?;
        Ok(())
    }

    /// Every invite, newest first. Open or requested ones past their expiry
    /// are reported as expired.
    pub fn invites(&self, now_ms: i64) -> Result<Vec<Invite>> {
        let mut statement = self.db.prepare(
            "SELECT token, label, created_ms, expires_ms, state, joiner, joiner_name, joiner_is_bot
             FROM invites ORDER BY created_ms DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<Vec<u8>>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<bool>>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (token, label, created_ms, expires_ms, state, joiner, joiner_name, joiner_is_bot) = row?;
            let joiner = joiner.map(to_public_key).transpose()?;
            let mut state = parse_state(&state)?;
            if matches!(state, InviteState::Open | InviteState::Requested) && expires_ms <= now_ms {
                state = InviteState::Expired;
            }
            Ok(Invite {
                token,
                label,
                created_ms,
                expires_ms,
                state,
                requester_safety_code: joiner.as_ref().map(safety_code),
                requester_id: joiner.map(|key| member_id(&key)),
                requester_name: joiner_name,
                requester_is_bot: joiner_is_bot.unwrap_or(false),
            })
        })
        .collect()
    }

    /// Tokens of invites still waiting for a request.
    pub fn open_invite_tokens(&self, now_ms: i64) -> Result<Vec<String>> {
        let mut statement = self
            .db
            .prepare("SELECT token FROM invites WHERE state = ? AND expires_ms > ?")?;
        let rows = statement.query_map(params![state_name(InviteState::Open), now_ms], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn record_join_request(
        &self,
        token: &str,
        joiner: &PublicKey,
        name: &str,
        is_bot: bool,
        request: &[u8],
    ) -> Result<()> {
        self.db.execute(
            "UPDATE invites SET state = ?, joiner = ?, joiner_name = ?, joiner_is_bot = ?, request = ?
             WHERE token = ? AND state = ?",
            params![
                state_name(InviteState::Requested),
                joiner.as_bytes(),
                name,
                is_bot,
                request,
                token,
                state_name(InviteState::Open)
            ],
        )?;
        Ok(())
    }

    /// The request waiting on `token`, if it is still awaiting a decision.
    /// Who asked to join with `token`, whatever the request's state.
    pub fn requester(&self, token: &str) -> Result<Option<PublicKey>> {
        let joiner: Option<Vec<u8>> = self
            .db
            .query_row("SELECT joiner FROM invites WHERE token = ?", [token], |row| {
                row.get(0)
            })
            .optional()?
            .flatten();
        joiner
            .map(|bytes| {
                let bytes: [u8; 32] = bytes
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("a stored requester key is malformed"))?;
                Ok(PublicKey::from_bytes(&bytes)?)
            })
            .transpose()
    }

    pub fn pending_request(&self, token: &str, now_ms: i64) -> Result<Option<PendingRequest>> {
        let row: Option<(Vec<u8>, Vec<u8>)> = self
            .db
            .query_row(
                "SELECT joiner, request FROM invites WHERE token = ? AND state = ? AND expires_ms > ?",
                params![token, state_name(InviteState::Requested), now_ms],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(joiner, request)| {
            Ok(PendingRequest {
                joiner: to_public_key(joiner)?,
                request,
            })
        })
        .transpose()
    }

    /// Moves an invite from one of `from` to `to`. Returns false if it was in
    /// none of them (already decided, revoked, or unknown).
    pub fn change_invite_state(&self, token: &str, from: &[InviteState], to: InviteState) -> Result<bool> {
        for state in from {
            let changed = self.db.execute(
                "UPDATE invites SET state = ?, request = NULL WHERE token = ? AND state = ?",
                params![state_name(to), token, state_name(*state)],
            )?;
            if changed == 1 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn put_delivery(&self, joiner: &PublicKey, token: &str, payload: &[u8]) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO deliveries (joiner, token, payload) VALUES (?, ?, ?)",
            params![joiner.as_bytes(), token, payload],
        )?;
        Ok(())
    }

    pub fn deliveries(&self) -> Result<Vec<(PublicKey, String, Vec<u8>)>> {
        let mut statement = self.db.prepare("SELECT joiner, token, payload FROM deliveries")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (joiner, token, payload) = row?;
            Ok((to_public_key(joiner)?, token, payload))
        })
        .collect()
    }

    pub fn delete_delivery(&self, joiner: &PublicKey) -> Result<()> {
        self.db
            .execute("DELETE FROM deliveries WHERE joiner = ?", [joiner.as_bytes()])?;
        Ok(())
    }
}
