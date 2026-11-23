//! The durable mailbox: any S3-compatible bucket. It only ever holds
//! ciphertext (op envelopes, MLS messages) plus public ids.
//!
//! Layout under the workspace prefix:
//!   ops/<author>/<first seq>-<last seq>   segments of envelopes
//!   ops/forks/<author>/<seq>/<hash>       proof an author signed two versions of one op
//!   mls/commit/<from epoch>               admin-signed commits, one per epoch
//!   invites/<token>/request               a joiner's request (via presigned link)
//!   invites/<token>/answer                the admin's answer (via presigned link)
//!   peers/<endpoint id>                   gossip bootstrap hints
//!   recovery/snapshot                     the admin's encrypted recovery snapshot
//!   files/<author>/<blob id>/<piece>      a shared file's encrypted pieces (see `blobs`)

use std::{collections::VecDeque, sync::Arc, time::Duration};

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
use tokio::task::JoinHandle;
use tracing::warn;

use crate::{
    ops::Envelope,
    views::{StorageProtection, member_id, parse_member_id},
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

/// The largest recovery snapshot restore reads.
pub const MAX_RECOVERY_SNAPSHOT_BYTES: u64 = MAX_OBJECT_SIZE;

/// How many segments one pull fetches at once: a newcomer reads a log's
/// whole history, and each fetch takes a round trip to the bucket.
const PARALLEL_SEGMENT_FETCHES: usize = 8;

/// An author's segments, fetched ahead a few at a time.
pub struct SegmentFetch {
    bucket: Bucket,
    author: PublicKey,
    listed: std::vec::IntoIter<ListedObject>,
    in_flight: VecDeque<(ListedObject, JoinHandle<Result<Option<Bytes>>>)>,
}

/// An object's key and version, as a listing showed it.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ListedObject {
    pub key: String,
    /// Changes whenever the object is replaced.
    pub version: String,
}

pub struct FetchedSegment {
    pub listed: ListedObject,
    pub envelopes: Vec<Envelope>,
}

impl SegmentFetch {
    /// Leaves out `objects` (by key and version); call before fetching starts.
    pub fn without(mut self, objects: &std::collections::HashSet<ListedObject>) -> Self {
        self.listed = std::mem::take(&mut self.listed)
            .filter(|listed| !objects.contains(listed))
            .collect::<Vec<_>>()
            .into_iter();
        self
    }

    /// The next segment, in order. An unreadable one, or one whose bytes
    /// don't match the hash in its name, comes back empty, so one bad object
    /// can't block everything after it.
    pub async fn next(&mut self) -> Result<Option<FetchedSegment>> {
        while self.in_flight.len() < PARALLEL_SEGMENT_FETCHES {
            let Some(listed) = self.listed.next() else {
                break;
            };
            let bucket = self.bucket.clone();
            let path = Path::from(listed.key.as_str());
            let fetching = tokio::spawn(async move { bucket.get(&path).await });
            self.in_flight.push_back((listed, fetching));
        }
        let Some((listed, fetching)) = self.in_flight.pop_front() else {
            return Ok(None);
        };
        let path = &listed.key;
        let named_hash = parse_segment_name(&self.author, path).and_then(|name| name.hash);
        let envelopes = match fetching.await.context("fetching a segment")?? {
            Some(bytes) if named_hash.is_some_and(|hash| blake3::hash(&bytes) != hash) => {
                warn!("skipping segment {path}: its bytes don't match its name");
                Vec::new()
            }
            Some(bytes) => postcard::from_bytes(&bytes).unwrap_or_else(|error| {
                warn!("skipping unreadable segment {path}: {error}");
                Vec::new()
            }),
            None => Vec::new(),
        };
        Ok(Some(FetchedSegment { listed, envelopes }))
    }
}

impl Drop for SegmentFetch {
    /// A pull that stops early leaves nothing fetching in the background.
    fn drop(&mut self) {
        for (_, fetching) in &self.in_flight {
            fetching.abort();
        }
    }
}

/// Splits a log's run of envelopes, in order, into segments that each stay
/// under the size readers accept. A segment holds a count (at most 10 bytes)
/// and then the envelopes back to back.
pub fn split_into_segments(envelopes: &[Envelope]) -> Result<Vec<&[Envelope]>> {
    const COUNT_BYTES: usize = 10;
    let limit = usize::try_from(MAX_OBJECT_SIZE)?;
    let mut segments = Vec::new();
    let (mut start, mut size) = (0, COUNT_BYTES);
    for (index, envelope) in envelopes.iter().enumerate() {
        let envelope_size = postcard::to_stdvec(envelope)?.len();
        ensure!(
            COUNT_BYTES + envelope_size <= limit,
            "op {} is too large to upload",
            envelope.seq
        );
        if size + envelope_size > limit {
            segments.push(&envelopes[start..index]);
            (start, size) = (index, COUNT_BYTES);
        }
        size += envelope_size;
    }
    if start < envelopes.len() {
        segments.push(&envelopes[start..]);
    }
    Ok(segments)
}

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

const FORKS_PREFIX: &str = "ops/forks";

/// A fork proof object in the bucket, as listed.
pub struct ListedProof {
    pub author: PublicKey,
    pub seq: u64,
    pub path: String,
    /// Changes whenever the object is written again.
    pub version: String,
}
/// Two versions as large as a segment may hold, and their framing.
pub const MAX_FORK_PROOF_BYTES: u64 = 2 * MAX_OBJECT_SIZE + 4096;

pub fn fork_proof_path(author: &PublicKey, seq: u64, proof: &[u8]) -> String {
    format!(
        "{FORKS_PREFIX}/{}/{}/{}",
        member_id(author),
        padded(seq),
        blake3::hash(proof).to_hex()
    )
}

/// What a segment's name says.
#[derive(Debug, PartialEq)]
pub struct SegmentName {
    pub first: u64,
    pub last: u64,
    /// Newer names end in a hash of the segment's bytes.
    pub hash: Option<blake3::Hash>,
}

/// Parses `key` if it is exactly a segment name of `author`'s:
/// `ops/<author>/<first>-<last>`, both padded, optionally followed by
/// `-<hash of the bytes, 64 lowercase hex digits>`.
pub fn parse_segment_name(author: &PublicKey, key: &str) -> Option<SegmentName> {
    let name = key.strip_prefix(&format!("ops/{}/", member_id(author)))?;
    let mut parts = name.split('-');
    let (first, last) = (parts.next()?, parts.next()?);
    let hash = match parts.next() {
        None => None,
        Some(hex) => {
            let is_lowercase_hex =
                hex.len() == 64 && hex.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
            Some(blake3::Hash::from_hex(is_lowercase_hex.then_some(hex)?).ok()?)
        }
    };
    if parts.next().is_some() {
        return None;
    }
    let parse = |number: &str| {
        let is_padded = number.len() == padded(0).len() && number.bytes().all(|byte| byte.is_ascii_digit());
        is_padded.then(|| number.parse::<u64>().ok()).flatten()
    };
    let (first, last) = (parse(first)?, parse(last)?);
    (first <= last).then_some(SegmentName { first, last, hash })
}

/// Changes whenever the object is replaced: an overwrite always changes the
/// ETag, and stores without one still change the size or time.
fn object_version(meta: &object_store::ObjectMeta) -> String {
    meta.e_tag
        .clone()
        .unwrap_or_else(|| format!("{}@{}", meta.size, meta.last_modified))
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
        self.get_at_most(path, MAX_OBJECT_SIZE).await
    }

    pub(super) async fn get_at_most(&self, path: &Path, limit: u64) -> Result<Option<Bytes>> {
        match self.store.get(path).await {
            Ok(result) if result.meta.size > limit => {
                warn!("ignoring oversized object {path} ({} bytes)", result.meta.size);
                Ok(None)
            }
            Ok(result) => Ok(Some(result.bytes().await?)),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) async fn put(&self, path: &Path, bytes: Vec<u8>) -> Result<()> {
        self.store.put(path, PutPayload::from(bytes)).await?;
        Ok(())
    }

    pub(super) async fn list(&self, prefix: &str, after: Option<&str>) -> Result<Vec<Path>> {
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

    /// Uploads `envelopes` as one segment; see [`split_into_segments`] for
    /// keeping it readable.
    pub async fn put_segment(&self, envelopes: &[Envelope]) -> Result<()> {
        let (Some(first), Some(last)) = (envelopes.first(), envelopes.last()) else {
            return Ok(());
        };
        let path = segment_path(&first.author, first.seq, last.seq);
        self.put(&path, postcard::to_stdvec(envelopes)?).await
    }

    /// Segments by `author` after `cursor`, fetched a few at a time and
    /// handed over in order, each with its key to resume from.
    pub async fn segments_after(&self, author: &PublicKey, cursor: Option<&str>) -> Result<SegmentFetch> {
        let prefix = Path::from(format!("ops/{}", member_id(author)));
        let mut stream = match cursor {
            Some(offset) => self.store.list_with_offset(Some(&prefix), &Path::from(offset)),
            None => self.store.list(Some(&prefix)),
        };
        let mut listed = Vec::new();
        while let Some(meta) = stream.next().await {
            let meta = meta?;
            listed.push(ListedObject {
                key: meta.location.to_string(),
                version: object_version(&meta),
            });
        }
        listed.sort_by(|one, other| one.key.cmp(&other.key));
        Ok(SegmentFetch {
            bucket: self.clone(),
            author: *author,
            listed: listed.into_iter(),
            in_flight: VecDeque::new(),
        })
    }

    /// Stores proof that `author` signed two versions of op `seq`. The proof
    /// checks itself, and its name holds its hash, so readers can tell when
    /// someone overwrote it.
    pub async fn put_fork_proof(&self, author: &PublicKey, seq: u64, proof: Vec<u8>) -> Result<()> {
        self.put(&Path::from(fork_proof_path(author, seq, &proof)), proof)
            .await
    }

    /// Every fork proof in the bucket. Names that don't parse are skipped.
    pub async fn fork_proofs(&self) -> Result<Vec<ListedProof>> {
        let prefix = Path::from(FORKS_PREFIX);
        let mut stream = self.store.list(Some(&prefix));
        let mut proofs = Vec::new();
        while let Some(meta) = stream.next().await {
            let meta = meta?;
            let parts: Vec<_> = meta
                .location
                .parts()
                .map(|part| part.as_ref().to_owned())
                .collect();
            let [_, _, author, seq, _] = parts.as_slice() else {
                continue;
            };
            if let (Ok(author), Ok(seq)) = (parse_member_id(author), seq.parse()) {
                let version = object_version(&meta);
                proofs.push(ListedProof {
                    author,
                    seq,
                    path: meta.location.to_string(),
                    version,
                });
            }
        }
        Ok(proofs)
    }

    /// Writes `bytes` at `path` whatever they are, as anyone holding the
    /// bucket keys could.
    #[cfg(test)]
    pub async fn put_raw(&self, path: &str, bytes: Vec<u8>) -> Result<()> {
        self.put(&Path::from(path), bytes).await
    }

    #[cfg(test)]
    pub async fn delete_raw(&self, path: &str) -> Result<()> {
        self.delete(&Path::from(path)).await
    }

    pub async fn fork_proof(&self, path: &str) -> Result<Option<Bytes>> {
        self.get_at_most(&Path::from(path), MAX_FORK_PROOF_BYTES).await
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

    pub(super) async fn delete(&self, path: &Path) -> Result<()> {
        match self.store.delete(path).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Refuses a snapshot larger than restore would read, so the admin
    /// learns now rather than when they need the kit.
    pub async fn put_recovery_snapshot(&self, sealed: Vec<u8>) -> Result<()> {
        let size = sealed.len() as u64;
        ensure!(
            size <= MAX_RECOVERY_SNAPSHOT_BYTES,
            "the recovery snapshot is too large ({} MB; restore reads at most {} MB), so the recovery kit can't restore this workspace",
            size.div_ceil(1024 * 1024),
            MAX_RECOVERY_SNAPSHOT_BYTES / (1024 * 1024)
        );
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

    /// Who announced themselves for gossip, and when they last did.
    pub async fn peers(&self) -> Result<Vec<PeerAnnouncement>> {
        let mut stream = self.store.list(Some(&Path::from("peers")));
        let mut announcements = Vec::new();
        while let Some(meta) = stream.next().await {
            let meta = meta?;
            if let Some(peer) = meta.location.filename().and_then(parse_member_path) {
                announcements.push(PeerAnnouncement {
                    peer,
                    announced_ms: meta.last_modified.timestamp_millis(),
                });
            }
        }
        Ok(announcements)
    }
}

/// A member's gossip address hint, rewritten each time they start and every
/// few hours while they stay online.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerAnnouncement {
    pub peer: PublicKey,
    pub announced_ms: i64,
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
    fn only_exact_segment_names_of_the_author_give_a_range() {
        let alice = iroh::SecretKey::generate().public();
        let bob = iroh::SecretKey::generate().public();
        let real = segment_path(&alice, 3, 5).to_string();
        let range = |author: &PublicKey, key: &str| {
            parse_segment_name(author, key).map(|named| (named.first, named.last))
        };
        assert_eq!(range(&alice, &real), Some((3, 5)));
        assert_eq!(range(&bob, &real), None, "someone else's prefix");

        let alice_ops = format!("ops/{}", member_id(&alice));
        let max = u64::MAX;
        assert_eq!(
            range(&alice, &format!("{alice_ops}/{}-{}", padded(max), padded(max))),
            Some((max, max))
        );
        for junk in [
            "z".to_owned(),
            format!("{}-{}", "9".repeat(20), "9".repeat(20)),
            format!("{}-{}", padded(5), padded(3)),
            "3-5".to_owned(),
            format!("{}-{}-extra", padded(3), padded(5)),
            format!("+{}-{}", &padded(3)[1..], padded(5)),
        ] {
            assert_eq!(range(&alice, &format!("{alice_ops}/{junk}")), None, "{junk}");
        }
    }

    /// Newer segment names may end in a hash of the segment's bytes; older
    /// readers must already take them, so mixed versions keep working.
    #[test]
    fn segment_names_may_end_in_a_hash_of_their_bytes() {
        let alice = iroh::SecretKey::generate().public();
        let alice_ops = format!("ops/{}", member_id(&alice));
        let hash = blake3::hash(b"segment bytes");
        let range = format!("{}-{}", padded(3), padded(5));
        let named = parse_segment_name(&alice, &format!("{alice_ops}/{range}-{}", hash.to_hex())).unwrap();
        assert_eq!((named.first, named.last, named.hash), (3, 5, Some(hash)));
        assert_eq!(
            parse_segment_name(&alice, &format!("{alice_ops}/{range}"))
                .unwrap()
                .hash,
            None
        );

        let upper = hash.to_hex().to_uppercase();
        let short = &hash.to_hex()[..63];
        for junk in [upper.as_str(), short, "", "not-hex"] {
            assert!(
                parse_segment_name(&alice, &format!("{alice_ops}/{range}-{junk}")).is_none(),
                "{junk}"
            );
        }
        assert!(parse_segment_name(&alice, &format!("{alice_ops}/{range}-{}-more", hash.to_hex())).is_none());
    }

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

    /// Counts GETs in flight; each waits a little, so overlapping ones show.
    #[derive(Debug, Default)]
    struct SlowStore {
        inner: object_store::memory::InMemory,
        in_flight: std::sync::atomic::AtomicUsize,
        most_in_flight: std::sync::atomic::AtomicUsize,
        started: std::sync::atomic::AtomicUsize,
    }

    impl std::fmt::Display for SlowStore {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("slow test store")
        }
    }

    #[async_trait::async_trait]
    impl ObjectStore for SlowStore {
        async fn put_opts(
            &self,
            location: &Path,
            payload: PutPayload,
            opts: object_store::PutOptions,
        ) -> object_store::Result<object_store::PutResult> {
            self.inner.put_opts(location, payload, opts).await
        }

        async fn put_multipart_opts(
            &self,
            location: &Path,
            opts: object_store::PutMultipartOptions,
        ) -> object_store::Result<Box<dyn object_store::MultipartUpload>> {
            self.inner.put_multipart_opts(location, opts).await
        }

        async fn get_opts(
            &self,
            location: &Path,
            options: object_store::GetOptions,
        ) -> object_store::Result<object_store::GetResult> {
            use std::sync::atomic::Ordering;
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.most_in_flight.fetch_max(now, Ordering::SeqCst);
            // Earlier requests finish later, so handing them over in order
            // can't come from the order they finish in.
            let started = self.started.fetch_add(1, Ordering::SeqCst) as u64;
            tokio::time::sleep(std::time::Duration::from_millis(
                60u64.saturating_sub(started * 2),
            ))
            .await;
            let got = self.inner.get_opts(location, options).await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            got
        }

        fn delete_stream(
            &self,
            locations: futures_lite::stream::Boxed<object_store::Result<Path>>,
        ) -> futures_lite::stream::Boxed<object_store::Result<Path>> {
            self.inner.delete_stream(locations)
        }

        fn list(
            &self,
            prefix: Option<&Path>,
        ) -> futures_lite::stream::Boxed<object_store::Result<object_store::ObjectMeta>> {
            self.inner.list(prefix)
        }

        async fn list_with_delimiter(
            &self,
            prefix: Option<&Path>,
        ) -> object_store::Result<object_store::ListResult> {
            self.inner.list_with_delimiter(prefix).await
        }

        async fn copy_opts(
            &self,
            from: &Path,
            to: &Path,
            options: object_store::CopyOptions,
        ) -> object_store::Result<()> {
            self.inner.copy_opts(from, to, options).await
        }
    }

    /// A newcomer reads an author's history segment by segment: several at
    /// once, still handed over in order.
    #[tokio::test]
    async fn segments_are_fetched_several_at_a_time_in_order() {
        let store = Arc::new(SlowStore::default());
        let bucket = Bucket::without_signing(store.clone());
        let author = iroh::SecretKey::generate().public();
        let empty = postcard::to_stdvec(&Vec::<Envelope>::new()).unwrap();
        let mut written = Vec::new();
        for segment in 0..20 {
            let path = segment_path(&author, segment * 10 + 1, segment * 10 + 10);
            bucket.put(&path, empty.clone()).await.unwrap();
            written.push(path.to_string());
        }

        let mut fetching = bucket.segments_after(&author, None).await.unwrap();
        let mut fetched = Vec::new();
        while let Some(segment) = fetching.next().await.unwrap() {
            fetched.push(segment.listed.key);
        }

        assert_eq!(fetched, written);
        let most = store.most_in_flight.load(std::sync::atomic::Ordering::SeqCst);
        assert!((2..=PARALLEL_SEGMENT_FETCHES).contains(&most), "{most} at once");
    }

    #[tokio::test]
    async fn a_recovery_snapshot_too_large_to_read_back_is_refused() {
        let bucket = Bucket::without_signing(Arc::new(object_store::memory::InMemory::new()));
        let largest = MAX_RECOVERY_SNAPSHOT_BYTES as usize;
        bucket.put_recovery_snapshot(vec![7; largest]).await.unwrap();
        let stored = bucket.recovery_snapshot().await.unwrap().map(|bytes| bytes.len());
        assert_eq!(stored, Some(largest));

        let refused = bucket
            .put_recovery_snapshot(vec![7; largest + 1])
            .await
            .unwrap_err();
        assert!(format!("{refused:#}").contains("too large"), "{refused:#}");
        let stored = bucket.recovery_snapshot().await.unwrap().map(|bytes| bytes.len());
        assert_eq!(stored, Some(largest), "the last good snapshot stays");
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

    #[test]
    fn a_long_run_splits_into_readable_segments_in_order() {
        let author = iroh::SecretKey::generate().public();
        let envelope = |seq| Envelope {
            author,
            seq,
            epoch: 1,
            nonce: [0; 24],
            ciphertext: vec![0; 3 * 1024 * 1024],
        };
        let run: Vec<_> = (1..=25).map(envelope).collect();

        let segments = split_into_segments(&run).unwrap();

        assert!(segments.len() > 1);
        for segment in &segments {
            let size = postcard::to_stdvec(segment).unwrap().len();
            assert!(size as u64 <= MAX_OBJECT_SIZE, "a segment of {size} bytes");
        }
        let seqs: Vec<u64> = segments.concat().iter().map(|envelope| envelope.seq).collect();
        assert_eq!(seqs, (1..=25).collect::<Vec<_>>());
        assert_eq!(split_into_segments(&run[..2]).unwrap().len(), 1);
    }
}
