//! The signed "I'm in this huddle" or "I left" a device sends. It travels
//! over gossip for public channels and directly to a channel's members for
//! private ones, is kept in memory only, and is never stored.

use anyhow::{Context, Result, ensure};
use iroh::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};

use crate::ops::MAX_ID_CHARS;

const SIGNATURE_DOMAIN: &str = "shyspace huddle v1";
/// Our own number for what the media layer sends. Peers with another
/// number can't hear each other, so the app asks to update instead of
/// failing silently.
pub const MEDIA_WIRE: u16 = 1;
/// Real beacons are about 300 bytes; anything much larger is refused unread.
pub const MAX_BEACON_BYTES: usize = 1024;

/// One join: names the media path and orders the beacons sent within it.
pub type SessionId = [u8; 16];

/// How listeners reach a participant's media.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reach {
    /// Each listener connects to the participant.
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BeaconState {
    Joined {
        camera: bool,
        screen: bool,
        reach: Reach,
    },
    Left,
}

/// What a beacon says, before it is signed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeaconBody {
    pub member: PublicKey,
    /// The device. One member per device today (#53).
    pub endpoint: PublicKey,
    pub channel: String,
    pub session: SessionId,
    /// Counts beacons within the session, so ordering needs no clock.
    pub seq: u64,
    pub sent_ms: i64,
    pub state: BeaconState,
    pub media_wire: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Beacon {
    pub body: BeaconBody,
    pub signature: Vec<u8>,
}

impl Beacon {
    /// Signs `body` with the member's identity, for this workspace only.
    pub fn sign(identity: &SecretKey, workspace_id: &str, body: BeaconBody) -> Result<Self> {
        ensure!(
            body.member == identity.public(),
            "a beacon is signed by its own member"
        );
        let bytes = signed_bytes(workspace_id, &body)?;
        Ok(Self {
            signature: identity.sign(&bytes).to_bytes().to_vec(),
            body,
        })
    }

    /// Checks the signature and the shape. Membership, timing and order are
    /// the receiver's to check.
    pub fn verify(&self, workspace_id: &str) -> Result<()> {
        let body = &self.body;
        ensure!(
            body.member == body.endpoint,
            "a beacon's device must be its member's own"
        );
        ensure!(
            !body.channel.is_empty() && body.channel.len() <= MAX_ID_CHARS,
            "a beacon names no real channel"
        );
        let signature: [u8; 64] = self
            .signature
            .as_slice()
            .try_into()
            .context("malformed beacon signature")?;
        body.member
            .verify(
                &signed_bytes(workspace_id, body)?,
                &Signature::from_bytes(&signature),
            )
            .context("bad beacon signature")
    }

    /// Where the participant publishes their media in this session. Random
    /// per join, so it names no channel and can't be reused.
    pub fn media_path_name(&self) -> String {
        media_path_name(&self.body.session)
    }
}

pub fn media_path_name(session: &SessionId) -> String {
    format!("h-{}", hex::encode(session))
}

fn signed_bytes(workspace_id: &str, body: &BeaconBody) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(SIGNATURE_DOMAIN, workspace_id, body))?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn body(
        identity: &SecretKey,
        channel: &str,
        session: SessionId,
        seq: u64,
        sent_ms: i64,
    ) -> BeaconBody {
        BeaconBody {
            member: identity.public(),
            endpoint: identity.public(),
            channel: channel.into(),
            session,
            seq,
            sent_ms,
            state: BeaconState::Joined {
                camera: false,
                screen: false,
                reach: Reach::Direct,
            },
            media_wire: MEDIA_WIRE,
        }
    }

    #[test]
    fn a_signed_beacon_verifies_for_its_workspace_only() {
        let ada = SecretKey::generate();
        let beacon = Beacon::sign(&ada, "w1", body(&ada, "general", [1; 16], 0, 5)).unwrap();
        beacon.verify("w1").unwrap();
        assert!(beacon.verify("w2").is_err());
    }

    #[test]
    fn a_changed_field_breaks_the_signature() {
        let ada = SecretKey::generate();
        let beacon = Beacon::sign(&ada, "w1", body(&ada, "general", [1; 16], 0, 5)).unwrap();
        let mut moved = beacon.clone();
        moved.body.channel = "secret-plans".into();
        assert!(moved.verify("w1").is_err());
        let mut later = beacon;
        later.body.seq = 9;
        assert!(later.verify("w1").is_err());
    }

    #[test]
    fn nobody_signs_a_beacon_for_someone_else() {
        let (ada, bob) = (SecretKey::generate(), SecretKey::generate());
        assert!(Beacon::sign(&bob, "w1", body(&ada, "general", [1; 16], 0, 5)).is_err());
        let mut borrowed = Beacon::sign(&bob, "w1", body(&bob, "general", [1; 16], 0, 5)).unwrap();
        borrowed.body.member = ada.public();
        borrowed.body.endpoint = ada.public();
        assert!(borrowed.verify("w1").is_err());
    }

    #[test]
    fn the_media_path_comes_from_the_session_not_the_channel() {
        let ada = SecretKey::generate();
        let first = Beacon::sign(&ada, "w1", body(&ada, "general", [1; 16], 0, 5)).unwrap();
        let second = Beacon::sign(&ada, "w1", body(&ada, "general", [2; 16], 0, 6)).unwrap();
        assert_ne!(first.media_path_name(), second.media_path_name());
        assert!(!first.media_path_name().contains("general"));
    }

    #[test]
    fn a_beacon_fits_well_under_the_limit() {
        let ada = SecretKey::generate();
        let beacon = Beacon::sign(&ada, "w1", body(&ada, &"c".repeat(120), [1; 16], 9, 5)).unwrap();
        assert!(postcard::to_stdvec(&beacon).unwrap().len() < MAX_BEACON_BYTES / 2);
    }
}
