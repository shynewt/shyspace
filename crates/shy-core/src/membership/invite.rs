//! Invites. A code is single-use, expires, and carries no credentials: only
//! two presigned links, one to post a join request and one to fetch the
//! admin's answer. Bucket credentials reach the newcomer inside the encrypted
//! answer, after the admin approves.

use std::future::Future;

use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use super::{
    device::{BoundFreeze, DeviceRecord},
    keyring::KeyHistory,
};
use crate::transport::bucket::BucketConfig;

const PREFIX: &str = "shy3_";
/// Invites from versions that posted join requests in the clear.
const OLDER_PREFIX: &str = "shy2_";

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
    /// The admin's key for this invite's join request (see `request_seal`).
    pub request_key: Vec<u8>,
    /// The admin's app signs every acceptance, so the joiner ignores an
    /// unsigned one: someone with the bucket keys could otherwise strip or
    /// change the answer and spend the joiner's key. False for invites
    /// made before answers were signed.
    pub signs_acceptance: bool,
}

/// An invite made before acceptances were signed.
#[derive(Deserialize)]
struct InviteBeforeSigning {
    workspace_id: String,
    workspace_name: String,
    admin: PublicKey,
    admin_mls_key: Vec<u8>,
    token: String,
    expires_ms: i64,
    request_url: String,
    answer_url: String,
    request_key: Vec<u8>,
}

impl Invite {
    pub fn encode(&self) -> Result<String> {
        Ok(format!(
            "{PREFIX}{}",
            URL_SAFE_NO_PAD.encode(postcard::to_stdvec(self)?)
        ))
    }

    pub fn decode(code: &str) -> Result<Self> {
        let code = code.trim();
        ensure!(
            !code.starts_with(OLDER_PREFIX),
            "This invite was made by an older version of shyspace. Ask for a new one."
        );
        let payload = code.strip_prefix(PREFIX).context("not a shyspace invite code")?;
        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .context("invite code is damaged")?;
        if let Ok(invite) = postcard::from_bytes::<Self>(&bytes) {
            return Ok(invite);
        }
        let older: InviteBeforeSigning = postcard::from_bytes(&bytes).context("invite code is damaged")?;
        Ok(Self {
            workspace_id: older.workspace_id,
            workspace_name: older.workspace_name,
            admin: older.admin,
            admin_mls_key: older.admin_mls_key,
            token: older.token,
            expires_ms: older.expires_ms,
            request_url: older.request_url,
            answer_url: older.answer_url,
            request_key: older.request_key,
            signs_acceptance: false,
        })
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
    /// The joiner reads a signed acceptance; older apps can't.
    pub reads_signed_answers: bool,
}

/// A join request from an app that couldn't read a signed acceptance.
#[derive(Deserialize)]
struct JoinRequestBeforeSigning {
    key_package: Vec<u8>,
    binding: Vec<u8>,
    name: String,
    is_bot: bool,
}

impl JoinRequest {
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if let Ok(request) = postcard::from_bytes::<Self>(bytes) {
            return Ok(request);
        }
        let older: JoinRequestBeforeSigning = postcard::from_bytes(bytes)?;
        Ok(Self {
            key_package: older.key_package,
            binding: older.binding,
            name: older.name,
            is_bot: older.is_bot,
            reads_signed_answers: false,
        })
    }
}

/// The admin's answer to a join request.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Answer {
    /// An MLS Welcome, and the grant encrypted for the group (the newcomer can
    /// decrypt it once the Welcome has made them a member). Unsigned: sent
    /// only to apps that can't read [`Answer::AcceptedSigned`].
    Accepted { welcome: Vec<u8>, sealed_grant: Vec<u8> },
    /// Signed by the admin (see [`declined_bytes`]): an unsigned one could
    /// make a joiner throw away the key the admin is about to approve.
    Rejected { signature: Vec<u8> },
    /// The acceptance, signed by the admin (see [`accepted_bytes`]). Once
    /// the workspace has a linked device, the Welcome needs the device map's
    /// digest as a pre-shared key, so the answer carries it, and the sealed
    /// grant is a [`GrantWithDevices`].
    AcceptedSigned {
        welcome: Vec<u8>,
        sealed_grant: Vec<u8>,
        map_digest: Option<[u8; 32]>,
        signature: Vec<u8>,
    },
}

/// What the admin signs to accept `requester`'s request. Checked before the
/// Welcome is used: MLS deletes the joiner's key as it reads the Welcome, so
/// a swapped digest or grant would leave the real answer nothing to open.
pub(crate) fn accepted_bytes(
    workspace_id: &str,
    token: &str,
    requester: &PublicKey,
    map_digest: Option<&[u8; 32]>,
    welcome: &[u8],
    sealed_grant: &[u8],
) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(
        "shyspace join answer v1",
        workspace_id,
        token,
        requester.as_bytes(),
        map_digest,
        blake3::hash(welcome).as_bytes(),
        blake3::hash(sealed_grant).as_bytes(),
    ))?)
}

/// What the admin signs to decline `requester`'s request to join with `token`.
pub(crate) fn declined_bytes(workspace_id: &str, token: &str, requester: &PublicKey) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(
        "shyspace join declined v1",
        workspace_id,
        token,
        requester.as_bytes(),
    ))?)
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Grant {
    pub history: KeyHistory,
    pub bucket: BucketConfig,
    /// When `bucket` was last changed, so older changes replayed later
    /// can't overwrite it.
    pub bucket_lamport: u64,
}

/// The grant for a workspace that has linked devices: also the device map
/// and the freezes its digest covers, which the newcomer checks against the
/// digest in the answer before storing anything.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct GrantWithDevices {
    pub grant: Grant,
    pub devices: Vec<DeviceRecord>,
    pub bound_frozen: Vec<BoundFreeze>,
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
            request_key: vec![2; 32],
            signs_acceptance: true,
        };
        assert_eq!(Invite::decode(&invite.encode().unwrap()).unwrap(), invite);
        assert!(Invite::decode("hello").is_err());
        assert!(ensure_not_expired(&invite, 9).is_ok());
        assert!(ensure_not_expired(&invite, 10).is_err());
    }

    #[test]
    fn an_invite_from_an_older_version_says_to_ask_for_a_new_one() {
        let error = Invite::decode("shy2_AAAA").unwrap_err().to_string();
        assert!(error.contains("older version"), "{error}");
    }

    #[test]
    fn an_invite_or_request_from_before_signed_answers_still_reads() {
        #[derive(Serialize)]
        struct OlderInvite<'a> {
            workspace_id: &'a str,
            workspace_name: &'a str,
            admin: PublicKey,
            admin_mls_key: Vec<u8>,
            token: &'a str,
            expires_ms: i64,
            request_url: &'a str,
            answer_url: &'a str,
            request_key: Vec<u8>,
        }
        let older = OlderInvite {
            workspace_id: "abc",
            workspace_name: "Team",
            admin: iroh::SecretKey::generate().public(),
            admin_mls_key: vec![1; 32],
            token: "t",
            expires_ms: 10,
            request_url: "",
            answer_url: "",
            request_key: vec![2; 32],
        };
        let code = format!(
            "{PREFIX}{}",
            URL_SAFE_NO_PAD.encode(postcard::to_stdvec(&older).unwrap())
        );
        let invite = Invite::decode(&code).unwrap();
        assert_eq!(invite.token, "t");
        assert!(!invite.signs_acceptance);

        let request = postcard::to_stdvec(&(vec![1u8], vec![2u8], "Bob", false)).unwrap();
        let request = JoinRequest::decode(&request).unwrap();
        assert_eq!(request.name, "Bob");
        assert!(!request.reads_signed_answers);
    }
}
