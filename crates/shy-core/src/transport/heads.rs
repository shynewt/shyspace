//! Each author's head: a tiny object holding the last seq they uploaded, so
//! one listing of `heads/` tells a reader whose log moved. A hint only:
//! anyone with the bucket keys can change or delete it, and readers still
//! verify every op and list segments past what they hold.

use anyhow::{Context, Result};
use futures_lite::StreamExt;
use iroh::PublicKey;
use object_store::{ObjectStoreExt, PutPayload, path::Path};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::bucket::{Bucket, object_version};
use crate::views::{member_id, parse_member_id};

const HEADS_PREFIX: &str = "heads";
/// Far more than a head needs; anything bigger is junk.
const MAX_HEAD_BYTES: u64 = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Head {
    pub last_seq: u64,
    /// Which bucket key the author uses (`key_tag`), so the admin can see
    /// who has moved to a new one.
    pub key: [u8; 8],
}

/// A short tag naming a bucket key by its access key id (not a secret).
pub fn key_tag(access_key_id: &str) -> [u8; 8] {
    let hash = blake3::hash(access_key_id.as_bytes());
    let mut tag = [0; 8];
    tag.copy_from_slice(&hash.as_bytes()[..8]);
    tag
}

pub struct ListedHead {
    pub author: PublicKey,
    /// Changes whenever the head is replaced.
    pub version: String,
}

fn head_path(author: &PublicKey) -> Path {
    Path::from(format!("{HEADS_PREFIX}/{}", member_id(author)))
}

impl Bucket {
    /// Writes `author`'s head and returns its version, as a listing will
    /// show it (the ETag; stores without one get the bytes' hash).
    pub async fn put_head(&self, author: &PublicKey, head: Head) -> Result<String> {
        let bytes = postcard::to_stdvec(&head)?;
        let fallback = blake3::hash(&bytes).to_hex().to_string();
        let written = self
            .store()
            .put(&head_path(author), PutPayload::from(bytes))
            .await
            .context("writing our head")?;
        Ok(written.e_tag.unwrap_or(fallback))
    }

    /// Every head in the bucket. Names that aren't a member id are skipped.
    pub async fn heads(&self) -> Result<Vec<ListedHead>> {
        let mut listing = self.store().list(Some(&Path::from(HEADS_PREFIX)));
        let mut heads = Vec::new();
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            let Some(author) = meta
                .location
                .filename()
                .and_then(|name| parse_member_id(name).ok())
            else {
                continue;
            };
            heads.push(ListedHead {
                author,
                version: object_version(&meta),
            });
        }
        Ok(heads)
    }

    /// `author`'s head, if there is a readable one.
    pub async fn head(&self, author: &PublicKey) -> Result<Option<Head>> {
        let Some(bytes) = self.get_at_most(&head_path(author), MAX_HEAD_BYTES).await? else {
            return Ok(None);
        };
        match postcard::from_bytes(&bytes) {
            Ok(head) => Ok(Some(head)),
            Err(error) => {
                warn!("ignoring an unreadable head of {author}: {error}");
                Ok(None)
            }
        }
    }

    #[cfg(test)]
    pub async fn delete_head(&self, author: &PublicKey) -> Result<()> {
        self.delete(&head_path(author)).await
    }
}
