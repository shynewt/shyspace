//! An invite code carries everything a newcomer needs: where the bucket is,
//! who the admin is, and a token the admin will accept. Treat it as a secret.

use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use crate::transport::bucket::BucketConfig;

const PREFIX: &str = "shy1_";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Invite {
    pub workspace_id: String,
    pub workspace_name: String,
    pub admin: PublicKey,
    /// The admin's MLS signature key, so a joiner can tell a genuine Welcome
    /// from one forged by someone else with bucket access.
    pub admin_mls_key: Vec<u8>,
    pub token: String,
    pub bucket: BucketConfig,
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
            bucket: BucketConfig {
                endpoint: "http://localhost:9000".into(),
                region: "us-east-1".into(),
                bucket: "shy".into(),
                access_key_id: "a".into(),
                secret_access_key: "s".into(),
            },
        };
        assert_eq!(Invite::decode(&invite.encode().unwrap()).unwrap(), invite);
        assert!(Invite::decode("hello").is_err());
    }
}
