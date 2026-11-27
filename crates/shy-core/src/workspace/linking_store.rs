//! What a new device keeps while it links, in its own store beside the
//! folder it will finish in: the code, its request and the one-time secret
//! (deleted once the hand-over opens), so a restart picks the link up where
//! it was. The same store then holds what the device reads.

use anyhow::{Context, Result};
use iroh::SecretKey;
use zeroize::Zeroizing;

use super::{DataDir, lifecycle::prepare_empty_dir};
use crate::{
    identity,
    membership::link::{LinkCode, LinkRequest, Token},
    ops::now_ms,
    storage::Store,
};

pub(crate) const LINKING_SUFFIX: &str = ".linking";
const LINK_CODE_META: &str = "link_code";
const LINK_REQUEST_META: &str = "link_request";
pub(crate) const HANDOVER_SECRET_META: &str = "link_handover_secret";
const LINK_STARTED_META: &str = "link_started_ms";
const LINK_FINISHING_META: &str = "link_finishing";
const LINK_ENDED_META: &str = "link_ended";

/// How long a new device waits to be added before it wipes what it holds.
pub(crate) const LINKING_LIFETIME_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// A link this device started, as kept.
pub(super) struct KeptLink {
    pub code: LinkCode,
    pub request: LinkRequest,
    /// Gone once the hand-over opened.
    pub handover_secret: Option<Zeroizing<Vec<u8>>>,
    pub started_ms: i64,
    /// Set just before joining: MLS spends this device's key as it reads
    /// the welcome, so a join that began can't run again.
    pub is_finishing: bool,
    /// Wiped while something still held the store open: it goes at the
    /// next start.
    pub is_ended: bool,
}

impl KeptLink {
    pub fn is_expired(&self, now_ms: i64) -> bool {
        now_ms >= self.started_ms + LINKING_LIFETIME_MS
    }
}

impl DataDir {
    /// Where a link into this folder keeps its state and what it reads,
    /// under a key of its own.
    pub(crate) fn linking(&self) -> DataDir {
        let mut path = self.path.clone().into_os_string();
        path.push(LINKING_SUFFIX);
        DataDir::new(path, self.keys.beside(LINKING_SUFFIX))
    }

    /// Starts the linking store for a new link, replacing an earlier one.
    pub(super) async fn keep_link(
        &self,
        identity: &SecretKey,
        code: &LinkCode,
        request: &LinkRequest,
        handover_secret: &[u8],
    ) -> Result<()> {
        self.forget_reading().await?;
        prepare_empty_dir(&self.path)?;
        let local = self.open_local().await?;
        let store = &local.store;
        identity::store(store, identity)?;
        store.atomically(|store| {
            store.set_meta(LINK_CODE_META, code.encode()?.as_bytes())?;
            store.set_meta(LINK_REQUEST_META, &postcard::to_stdvec(request)?)?;
            store.set_meta(HANDOVER_SECRET_META, handover_secret)?;
            store.set_meta(LINK_STARTED_META, &now_ms()?.to_le_bytes())
        })
    }

    /// The link kept here, if there is one.
    /// Deletes the linking store if it still holds the link for `token`:
    /// a newer link started in the same folder since is left alone.
    pub(crate) async fn forget_reading_of(&self, token: &Token) -> Result<()> {
        if !self.store_path().try_exists()? {
            return Ok(());
        }
        let local = self.open_local().await?;
        let is_that_link = read_kept_link(&local.store).is_ok_and(|kept| kept.code.token == *token);
        if !is_that_link {
            return Ok(());
        }
        self.forget_reading_locked(local.into_lock()).await
    }

    pub(super) async fn kept_link(&self) -> Result<Option<KeptLink>> {
        if !self.store_path().try_exists()? {
            return Ok(None);
        }
        let local = self.open_local().await?;
        read_kept_link(&local.store).map(Some)
    }
}

/// Whether `store` is a linking store, which opens only for reading.
pub(super) fn is_linking_store(store: &Store) -> Result<bool> {
    Ok(store.meta(LINK_CODE_META)?.is_some())
}

/// Marks the join as begun, in the linking store.
pub(super) fn mark_link_finishing(store: &Store) -> Result<()> {
    store.set_meta(LINK_FINISHING_META, &[1])
}

/// Marks the link as over, so the store goes even if something holds it
/// open now.
pub(super) fn mark_link_ended(store: &Store) -> Result<()> {
    store.set_meta(LINK_ENDED_META, &[1])
}

/// Makes the link kept in `data` look `age_ms` old.
#[cfg(test)]
pub(crate) async fn backdate_link(data: &DataDir, age_ms: i64) -> Result<()> {
    let started = now_ms()? - age_ms;
    data.open_local()
        .await?
        .store
        .set_meta(LINK_STARTED_META, &started.to_le_bytes())
}

fn read_kept_link(store: &Store) -> Result<KeptLink> {
    let meta = |key: &str| {
        store
            .meta(key)?
            .with_context(|| format!("a linking store without its {key}"))
    };
    let code = LinkCode::decode(std::str::from_utf8(&meta(LINK_CODE_META)?)?)?;
    let request = postcard::from_bytes(&meta(LINK_REQUEST_META)?).context("reading a kept link request")?;
    let started = meta(LINK_STARTED_META)?;
    Ok(KeptLink {
        code,
        request,
        handover_secret: store.meta(HANDOVER_SECRET_META)?.map(Zeroizing::new),
        started_ms: i64::from_le_bytes(
            started
                .as_slice()
                .try_into()
                .context("a kept link's start time is corrupt")?,
        ),
        is_finishing: store.meta(LINK_FINISHING_META)?.is_some(),
        is_ended: store.meta(LINK_ENDED_META)?.is_some(),
    })
}
