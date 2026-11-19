//! The durable mailbox: any S3-compatible bucket. It only ever holds
//! ciphertext (op envelopes, MLS messages) plus public ids.
//!
//! Layout under the workspace prefix:
//!   ops/<author>/<first seq>-<last seq>   segments of envelopes
//!   mls/commit/<from epoch>               admin-signed commits, one per epoch
//!   invites/<token>/request               a joiner's request (via presigned link)
//!   invites/<token>/answer                the admin's answer (via presigned link)
//!   peers/<endpoint id>                   gossip bootstrap hints

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use bytes::Bytes;
use futures_lite::StreamExt;
use iroh::PublicKey;
use object_store::{
    ObjectStore, ObjectStoreExt, PutPayload,
    aws::AmazonS3Builder,
    path::Path,
    prefix::PrefixStore,
    signer::{Method, Signer},
};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{ops::Envelope, views::member_id};

/// Objects larger than this are ignored, so a hostile writer can't make us
/// load arbitrary amounts of data into memory.
const MAX_OBJECT_SIZE: u64 = 32 * 1024 * 1024;

fn parse_member_path(name: &str) -> Option<PublicKey> {
    let bytes: [u8; 32] = hex::decode(name).ok()?.try_into().ok()?;
    PublicKey::from_bytes(&bytes).ok()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BucketConfig {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl BucketConfig {
    pub fn connect(&self, workspace_id: &str) -> Result<Bucket> {
        let s3 = AmazonS3Builder::new()
            .with_endpoint(&self.endpoint)
            .with_allow_http(self.endpoint.starts_with("http://"))
            .with_region(&self.region)
            .with_bucket_name(&self.bucket)
            .with_access_key_id(&self.access_key_id)
            .with_secret_access_key(&self.secret_access_key)
            .build()
            .context("invalid bucket settings")?;
        let s3 = Arc::new(s3);
        Ok(Bucket {
            store: Arc::new(PrefixStore::new(s3.clone(), workspace_id)),
            signing: Some(Signing {
                signer: s3,
                prefix: workspace_id.to_owned(),
            }),
        })
    }
}

/// Presigns links for people without credentials (invitees). The signer sees
/// full paths, so it needs the workspace prefix the store adds implicitly.
#[derive(Clone)]
struct Signing {
    signer: Arc<dyn Signer>,
    prefix: String,
}

#[derive(Clone)]
pub struct Bucket {
    store: Arc<dyn ObjectStore>,
    signing: Option<Signing>,
}

/// Presigned links to one invite's request and answer slots.
pub struct InviteLinks {
    pub request_url: String,
    pub answer_url: String,
}

fn invite_path(token: &str, slot: &str) -> Path {
    Path::from(format!("invites/{token}/{slot}"))
}

fn padded(number: u64) -> String {
    format!("{number:020}")
}

fn segment_path(author: &PublicKey, first: u64, last: u64) -> Path {
    Path::from(format!(
        "ops/{}/{}-{}",
        member_id(author),
        padded(first),
        padded(last)
    ))
}

impl Bucket {
    /// A bucket that can't presign links (for example in-memory, in tests).
    pub fn without_signing(store: Arc<dyn ObjectStore>) -> Self {
        Self { store, signing: None }
    }

    async fn get(&self, path: &Path) -> Result<Option<Bytes>> {
        match self.store.get(path).await {
            Ok(result) if result.meta.size > MAX_OBJECT_SIZE => {
                warn!("ignoring oversized object {path} ({} bytes)", result.meta.size);
                Ok(None)
            }
            Ok(result) => Ok(Some(result.bytes().await?)),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    async fn put(&self, path: &Path, bytes: Vec<u8>) -> Result<()> {
        self.store.put(path, PutPayload::from(bytes)).await?;
        Ok(())
    }

    async fn list(&self, prefix: &str, after: Option<&str>) -> Result<Vec<Path>> {
        let prefix = Path::from(prefix);
        let mut stream = match after {
            Some(offset) => self.store.list_with_offset(Some(&prefix), &Path::from(offset)),
            None => self.store.list(Some(&prefix)),
        };
        let mut paths = Vec::new();
        while let Some(meta) = stream.next().await {
            paths.push(meta?.location);
        }
        paths.sort();
        Ok(paths)
    }

    pub async fn put_segment(&self, envelopes: &[Envelope]) -> Result<()> {
        let (Some(first), Some(last)) = (envelopes.first(), envelopes.last()) else {
            return Ok(());
        };
        let path = segment_path(&first.author, first.seq, last.seq);
        self.put(&path, postcard::to_stdvec(envelopes)?).await
    }

    /// Segments by `author` after `cursor`, each with its key to resume
    /// from. Unreadable segments come back empty so one bad object can't
    /// block everything after it.
    pub async fn segments_after(
        &self,
        author: &PublicKey,
        cursor: Option<&str>,
    ) -> Result<Vec<(String, Vec<Envelope>)>> {
        let mut segments = Vec::new();
        for path in self.list(&format!("ops/{}", member_id(author)), cursor).await? {
            let envelopes = match self.get(&path).await? {
                Some(bytes) => postcard::from_bytes(&bytes).unwrap_or_else(|error| {
                    warn!("skipping unreadable segment {path}: {error}");
                    Vec::new()
                }),
                None => Vec::new(),
            };
            segments.push((path.to_string(), envelopes));
        }
        Ok(segments)
    }

    /// Commits are signed by the admin (checked by readers), so a plain put
    /// is enough: the admin re-uploads its latest commit if it goes missing.
    pub async fn put_commit(&self, from_epoch: u64, signed_commit: Vec<u8>) -> Result<()> {
        self.put(
            &Path::from(format!("mls/commit/{}", padded(from_epoch))),
            signed_commit,
        )
        .await
    }

    pub async fn commit(&self, from_epoch: u64) -> Result<Option<Bytes>> {
        self.get(&Path::from(format!("mls/commit/{}", padded(from_epoch))))
            .await
    }

    /// Links an invitee can use without credentials, valid for `valid_for`.
    pub async fn presign_invite(&self, token: &str, valid_for: Duration) -> Result<InviteLinks> {
        let signing = self.signing.as_ref().context("this bucket can't presign links")?;
        let full_path = |slot| Path::from(format!("{}/invites/{token}/{slot}", signing.prefix));
        let request = signing
            .signer
            .signed_url(Method::PUT, &full_path("request"), valid_for)
            .await?;
        let answer = signing
            .signer
            .signed_url(Method::GET, &full_path("answer"), valid_for)
            .await?;
        Ok(InviteLinks {
            request_url: request.to_string(),
            answer_url: answer.to_string(),
        })
    }

    pub async fn invite_request(&self, token: &str) -> Result<Option<Bytes>> {
        self.get(&invite_path(token, "request")).await
    }

    pub async fn delete_invite_request(&self, token: &str) -> Result<()> {
        self.delete(&invite_path(token, "request")).await
    }

    pub async fn delete_invite_answer(&self, token: &str) -> Result<()> {
        self.delete(&invite_path(token, "answer")).await
    }

    pub async fn put_invite_answer(&self, token: &str, answer: Vec<u8>) -> Result<()> {
        self.put(&invite_path(token, "answer"), answer).await
    }

    async fn delete(&self, path: &Path) -> Result<()> {
        match self.store.delete(path).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn announce_peer(&self, peer: &PublicKey) -> Result<()> {
        self.put(&Path::from(format!("peers/{}", member_id(peer))), Vec::new())
            .await
    }

    pub async fn peers(&self) -> Result<Vec<PublicKey>> {
        let paths = self.list("peers", None).await?;
        Ok(paths
            .iter()
            .filter_map(|path| path.filename())
            .filter_map(parse_member_path)
            .collect())
    }
}

/// What an invitee does through presigned links, done directly in tests
/// where the in-memory bucket can't presign.
#[cfg(test)]
impl Bucket {
    pub async fn put_invite_request(&self, token: &str, request: Vec<u8>) -> Result<()> {
        self.put(&invite_path(token, "request"), request).await
    }

    pub async fn invite_answer(&self, token: &str) -> Result<Option<Bytes>> {
        self.get(&invite_path(token, "answer")).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn presigned_invite_links_carry_no_secret() {
        let config = BucketConfig {
            endpoint: "http://localhost:9".into(),
            region: "us-east-1".into(),
            bucket: "team".into(),
            access_key_id: "AKIDEXAMPLE".into(),
            secret_access_key: "very-secret-key".into(),
        };
        let bucket = config.connect("workspace").unwrap();
        let links = bucket
            .presign_invite("token123", Duration::from_secs(60))
            .await
            .unwrap();
        for url in [&links.request_url, &links.answer_url] {
            assert!(url.contains("/team/workspace/invites/token123/"), "{url}");
            assert!(url.contains("X-Amz-Signature="), "{url}");
            assert!(!url.contains("very-secret-key"), "{url}");
        }
    }
}
