//! The durable mailbox: any S3-compatible bucket. It only ever holds
//! ciphertext (op envelopes, MLS messages) plus public peer ids.
//!
//! Layout under the workspace prefix:
//!   ops/<author>/<first seq>-<last seq>   segments of envelopes
//!   mls/commit/<from epoch>               admin commits, one per epoch
//!   mls/keyring/<epoch>                   epoch-key history for newcomers
//!   join/<invite token>/<joiner>          key packages awaiting the admin
//!   welcome/<joiner>                      the admin's answer
//!   peers/<endpoint id>                   gossip bootstrap hints

use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::Bytes;
use futures_lite::StreamExt;
use iroh::PublicKey;
use object_store::{
    ObjectStore, ObjectStoreExt, PutPayload, aws::AmazonS3Builder, path::Path, prefix::PrefixStore,
};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{ops::Envelope, views::member_id};

/// Objects larger than this are ignored, so a hostile writer can't make us
/// load arbitrary amounts of data into memory.
const MAX_OBJECT_SIZE: u64 = 32 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinRequest {
    pub key_package: Vec<u8>,
    /// The joiner's identity-key signature over their MLS signature key.
    pub binding: Vec<u8>,
}

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
        Ok(Bucket::new(Arc::new(PrefixStore::new(s3, workspace_id))))
    }
}

#[derive(Clone)]
pub struct Bucket {
    store: Arc<dyn ObjectStore>,
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
    pub fn new(store: Arc<dyn ObjectStore>) -> Self {
        Self { store }
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

    pub async fn commit_slots(&self) -> Result<Vec<u64>> {
        let paths = self.list("mls/commit", None).await?;
        Ok(paths
            .iter()
            .filter_map(|path| path.filename()?.parse().ok())
            .collect())
    }

    pub async fn commit(&self, from_epoch: u64) -> Result<Option<Bytes>> {
        self.get(&Path::from(format!("mls/commit/{}", padded(from_epoch))))
            .await
    }

    pub async fn put_keyring(&self, epoch: u64, sealed: Vec<u8>) -> Result<()> {
        self.put(&Path::from(format!("mls/keyring/{}", padded(epoch))), sealed)
            .await
    }

    pub async fn keyring(&self, epoch: u64) -> Result<Option<Bytes>> {
        self.get(&Path::from(format!("mls/keyring/{}", padded(epoch))))
            .await
    }

    pub async fn request_join(&self, token: &str, joiner: &PublicKey, request: &JoinRequest) -> Result<()> {
        let path = Path::from(format!("join/{token}/{}", member_id(joiner)));
        self.put(&path, postcard::to_stdvec(request)?).await
    }

    /// Pending join requests as `(path, joiner named by the path, request)`.
    /// Malformed ones are returned as `None` so the admin can delete them.
    pub async fn join_requests(&self, token: &str) -> Result<Vec<(Path, Option<(PublicKey, JoinRequest)>)>> {
        let mut requests = Vec::new();
        for path in self.list(&format!("join/{token}"), None).await? {
            let joiner = path.filename().and_then(parse_member_path);
            let request = match self.get(&path).await? {
                Some(bytes) => postcard::from_bytes::<JoinRequest>(&bytes).ok(),
                None => continue,
            };
            requests.push((path, joiner.zip(request)));
        }
        Ok(requests)
    }

    pub async fn delete(&self, path: &Path) -> Result<()> {
        self.store.delete(path).await?;
        Ok(())
    }

    pub async fn put_welcome(&self, joiner: &PublicKey, welcome: Vec<u8>) -> Result<()> {
        self.put(&Path::from(format!("welcome/{}", member_id(joiner))), welcome)
            .await
    }

    pub async fn welcome(&self, joiner: &PublicKey) -> Result<Option<Bytes>> {
        self.get(&Path::from(format!("welcome/{}", member_id(joiner))))
            .await
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
