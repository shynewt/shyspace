//! The durable mailbox: any S3-compatible bucket. It only ever holds
//! ciphertext (op envelopes, MLS messages) plus public ids.
//!
//! Layout under the workspace prefix:
//!   ops/<author>/<first seq>-<last seq>   segments of envelopes
//!   mls/commit/<from epoch>               admin-signed commits, one per epoch
//!   invites/<token>/request               a joiner's request (via presigned link)
//!   invites/<token>/answer                the admin's answer (via presigned link)
//!   peers/<endpoint id>                   gossip bootstrap hints
//!   recovery/snapshot                     the admin's encrypted recovery snapshot

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, ensure};
use bytes::Bytes;
use futures_lite::StreamExt;
use iroh::PublicKey;
use object_store::{
    GetOptions, ObjectStore, ObjectStoreExt, PutPayload,
    aws::AmazonS3Builder,
    path::Path,
    prefix::PrefixStore,
    signer::{Method, Signer},
};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{
    ops::Envelope,
    views::{StorageProtection, member_id},
};

const RECOVERY_SNAPSHOT: &str = "recovery/snapshot";
const ACCESS_CHECK: &str = "access-check";
const VERSIONING_PROBE: &str = "health/versioning-probe";
/// Beside the authors' logs, never inside one, so no log listing sees it.
/// One per member: a locked bucket keeps every probe for good.
const LOCK_PROBE_PREFIX: &str = "ops/lock-probe-";

/// Objects larger than this are ignored, so a hostile writer can't make us
/// load arbitrary amounts of data into memory.
const MAX_OBJECT_SIZE: u64 = 32 * 1024 * 1024;

fn parse_member_path(name: &str) -> Option<PublicKey> {
    let bytes: [u8; 32] = hex::decode(name).ok()?.try_into().ok()?;
    PublicKey::from_bytes(&bytes).ok()
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct BucketConfig {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

/// Never prints the secret key, so a stray `{:?}` can't leak it.
impl std::fmt::Debug for BucketConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BucketConfig")
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"<hidden>")
            .finish()
    }
}

/// Plain http is only for servers on this machine or the local network;
/// anywhere else the keys would cross the internet readable.
fn is_local_host(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    match host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<std::net::IpAddr>()
    {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback(),
        Err(_) => host == "localhost" || host.ends_with(".local") || !host.contains('.'),
    }
}

impl BucketConfig {
    /// The endpoint as a full URL. People often paste just the host (as
    /// providers show it), so a missing scheme means https.
    pub fn endpoint_url(&self) -> Result<String> {
        let trimmed = self.endpoint.trim();
        ensure!(!trimmed.is_empty(), "the bucket endpoint is empty");
        let with_scheme = if trimmed.contains("://") {
            trimmed.to_owned()
        } else {
            format!("https://{trimmed}")
        };
        let url = reqwest::Url::parse(&with_scheme)
            .with_context(|| format!("the bucket endpoint {:?} isn't a valid address", self.endpoint))?;
        ensure!(
            url.scheme() == "https" || (url.scheme() == "http" && is_local_host(&url)),
            "the bucket endpoint must use https:// (plain http:// only works for a server on your own network)"
        );
        ensure!(
            url.host_str().is_some_and(|host| !host.is_empty()),
            "the bucket endpoint has no host"
        );
        Ok(with_scheme.trim_end_matches('/').to_owned())
    }

    /// Checks the settings work (write, read back, delete) without creating
    /// anything that stays in the bucket.
    pub async fn check(&self) -> Result<()> {
        let scratch = format!("access-check-{}", hex::encode(rand::random::<[u8; 8]>()));
        self.connect(&scratch)?.check_access().await
    }

    pub fn connect(&self, workspace_id: &str) -> Result<Bucket> {
        let endpoint = self.endpoint_url()?;
        let s3 = AmazonS3Builder::new()
            .with_endpoint(&endpoint)
            .with_allow_http(endpoint.starts_with("http://"))
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

/// Turns bucket settings into a connected bucket for a workspace. The app
/// uses S3 ([`s3_connector`]); tests use an in-memory bucket.
pub type Connector = Arc<dyn Fn(&str, &BucketConfig) -> Result<Bucket> + Send + Sync>;

pub fn s3_connector() -> Connector {
    Arc::new(|workspace_id: &str, config: &BucketConfig| config.connect(workspace_id))
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

    /// Epochs that have a commit slot in the bucket, valid or not.
    pub async fn commit_epochs(&self) -> Result<Vec<u64>> {
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

    pub async fn put_recovery_snapshot(&self, sealed: Vec<u8>) -> Result<()> {
        self.put(&Path::from(RECOVERY_SNAPSHOT), sealed).await
    }

    pub async fn recovery_snapshot(&self) -> Result<Option<Bytes>> {
        self.get(&Path::from(RECOVERY_SNAPSHOT)).await
    }

    /// Writes, reads back and deletes a small object, so wrong credentials,
    /// bucket names or endpoints fail now with a clear error rather than
    /// later in the middle of something else.
    pub async fn check_access(&self) -> Result<()> {
        let path = Path::from(ACCESS_CHECK);
        let probe = rand::random::<[u8; 16]>().to_vec();
        self.put(&path, probe.clone())
            .await
            .context("can't write to the bucket")?;
        let read = self.get(&path).await.context("can't read from the bucket")?;
        ensure!(
            read.as_deref() == Some(probe.as_slice()),
            "the bucket didn't return what was written"
        );
        self.delete(&path).await.context("can't delete from the bucket")
    }

    /// Whether deleted or overwritten data can be brought back, proved
    /// rather than asked. Locked: the bucket refuses to delete this member's
    /// probe among the ops (a retention lock on the write-once `ops/` and
    /// `mls/`), which stops even someone with the keys. Versioned: an
    /// overwritten probe's first version still reads back, which undoes
    /// accidents but not someone with the keys, who can delete old versions.
    pub async fn check_protection(&self, member: &PublicKey) -> Result<StorageProtection> {
        if self.refuses_deleting_ops(member).await? {
            return Ok(StorageProtection::Locked);
        }
        if self.keeps_versions().await? {
            return Ok(StorageProtection::Versioned);
        }
        Ok(StorageProtection::Unprotected)
    }

    async fn refuses_deleting_ops(&self, member: &PublicKey) -> Result<bool> {
        let path = Path::from(format!("{LOCK_PROBE_PREFIX}{}", member_id(member)));
        let wrote = self.put(&path, rand::random::<[u8; 16]>().to_vec()).await;
        let exists = || async { self.get(&path).await.context("can't read from the bucket") };
        if wrote.is_err() {
            // A lock also refuses overwriting the probe left by the last check.
            return match exists().await?.is_some() {
                true => Ok(true),
                false => wrote.map(|()| false).context("can't write to the bucket"),
            };
        }
        // Only two refusals in a row with the probe still there prove a lock;
        // one failed delete could be the network.
        for _ in 0..2 {
            if self.store.delete(&path).await.is_ok() {
                return Ok(false);
            }
        }
        Ok(exists().await?.is_some())
    }

    async fn keeps_versions(&self) -> Result<bool> {
        let path = Path::from(VERSIONING_PROBE);
        let first_bytes = rand::random::<[u8; 16]>();
        let first = self
            .store
            .put(&path, PutPayload::from(first_bytes.to_vec()))
            .await
            .context("can't write to the bucket")?;
        self.put(&path, rand::random::<[u8; 16]>().to_vec())
            .await
            .context("can't write to the bucket")?;
        let Some(version) = first.version.filter(|version| version != "null") else {
            return Ok(false);
        };
        let options = GetOptions {
            version: Some(version),
            ..GetOptions::default()
        };
        match self.store.get_opts(&path, options).await {
            Ok(earlier) => Ok(earlier.bytes().await?.as_ref() == first_bytes),
            Err(object_store::Error::NotFound { .. } | object_store::Error::NotImplemented { .. }) => {
                Ok(false)
            }
            Err(error) => Err(error).context("can't read from the bucket"),
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
    pub async fn delete_commit(&self, from_epoch: u64) -> Result<()> {
        self.delete(&Path::from(format!("mls/commit/{}", padded(from_epoch))))
            .await
    }

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

    #[test]
    fn endpoints_get_a_scheme_and_bad_ones_are_refused() {
        let endpoint = |value: &str| {
            BucketConfig {
                endpoint: value.into(),
                ..test_config()
            }
            .endpoint_url()
        };
        assert_eq!(
            endpoint("s3.us-west-000.backblazeb2.com").unwrap(),
            "https://s3.us-west-000.backblazeb2.com"
        );
        assert_eq!(
            endpoint(" https://s3.example.com/ ").unwrap(),
            "https://s3.example.com"
        );
        assert_eq!(
            endpoint("http://localhost:9000").unwrap(),
            "http://localhost:9000"
        );
        assert!(endpoint("").is_err());
        assert!(endpoint("ftp://files.example.com").is_err());
        assert!(
            endpoint("http://s3.example.com").is_err(),
            "keys would cross the internet in the clear"
        );
        assert!(endpoint("http://192.168.1.20:9000").is_ok());
        assert!(endpoint("http://minio:9000").is_ok());
        assert!(endpoint("https://").is_err());
    }

    #[test]
    fn debug_output_hides_the_secret() {
        let printed = format!("{:?}", test_config());
        assert!(!printed.contains("very-secret-key"), "{printed}");
        assert!(printed.contains("AKIDEXAMPLE"));
    }

    #[tokio::test]
    async fn access_check_round_trips_through_the_bucket() {
        let bucket = Bucket::without_signing(Arc::new(object_store::memory::InMemory::new()));
        bucket.check_access().await.unwrap();
    }

    fn test_config() -> BucketConfig {
        BucketConfig {
            endpoint: "http://localhost:9".into(),
            region: "us-east-1".into(),
            bucket: "team".into(),
            access_key_id: "AKIDEXAMPLE".into(),
            secret_access_key: "very-secret-key".into(),
        }
    }

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
