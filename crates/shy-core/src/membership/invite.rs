//! Invites. A code is single-use, expires, and carries no credentials: only
//! two presigned links, one to post a join request and one to fetch the
//! admin's answer. Bucket credentials reach the newcomer inside the encrypted
//! answer, after the admin approves.

use std::future::Future;

use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use super::keyring::KeyHistory;
use crate::transport::bucket::BucketConfig;

const PREFIX: &str = "shy2_";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Invite {
    pub workspace_id: String,
    pub workspace_name: String,
    pub admin: PublicKey,
    /// The admin's MLS signature key, so a joiner can tell a genuine Welcome
    /// from one forged by someone else with bucket access.
    pub admin_mls_key: Vec<u8>,
    pub token: String,
    pub expires_ms: i64,
    pub request_url: String,
    pub answer_url: String,
}

impl Invite {
    pub fn encode(&self) -> Result<String> {
        Ok(format!(
            "{PREFIX}{}",
            URL_SAFE_NO_PAD.encode(postcard::to_stdvec(self)?)
        ))
    }

    pub fn decode(code: &str) -> Result<Self> {
        let payload = code
            .trim()
            .strip_prefix(PREFIX)
            .context("not a shyspace invite code")?;
        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .context("invite code is damaged")?;
        postcard::from_bytes(&bytes).context("invite code is damaged")
    }
}

/// What a joiner posts through the request link.
#[derive(Debug, Serialize, Deserialize)]
pub struct JoinRequest {
    pub key_package: Vec<u8>,
    /// The joiner's identity-key signature over their MLS signature key.
    pub binding: Vec<u8>,
    /// Shown to the admin when deciding; the joiner's profile op sets it for real.
    pub name: String,
    pub is_bot: bool,
}

/// The admin's answer to a join request.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Answer {
    /// An MLS Welcome, and the grant encrypted for the group (the newcomer can
    /// decrypt it once the Welcome has made them a member).
    Accepted {
        welcome: Vec<u8>,
        sealed_grant: Vec<u8>,
    },
    Rejected,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Grant {
    pub history: KeyHistory,
    pub bucket: BucketConfig,
    /// When `bucket` was last changed, so older changes replayed later
    /// can't overwrite it.
    pub bucket_lamport: u64,
}

/// How a joiner reaches the admin before having bucket credentials.
pub trait JoinMailbox: Send + Sync {
    fn post_request(&self, request: Vec<u8>) -> impl Future<Output = Result<()>> + Send;
    /// The admin's answer, once there is one.
    fn fetch_answer(&self) -> impl Future<Output = Result<Option<Vec<u8>>>> + Send;
}

pub(crate) fn ensure_not_expired(invite: &Invite, now_ms: i64) -> Result<()> {
    ensure!(
        now_ms < invite.expires_ms,
        "this invite has expired; ask for a new one"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let invite = Invite {
            workspace_id: "abc".into(),
            workspace_name: "Team".into(),
            admin: iroh::SecretKey::generate().public(),
            admin_mls_key: vec![1; 32],
            token: "t".into(),
            expires_ms: 10,
            request_url: "https://bucket.example/request?sig".into(),
            answer_url: "https://bucket.example/answer?sig".into(),
        };
        assert_eq!(Invite::decode(&invite.encode().unwrap()).unwrap(), invite);
        assert!(Invite::decode("hello").is_err());
        assert!(ensure_not_expired(&invite, 9).is_ok());
        assert!(ensure_not_expired(&invite, 10).is_err());
    }
}
