//! Asking a member for the storage settings, and answering (see
//! `sync/key_handoff.rs`). One request and its answer per connection, which
//! the members-only hooks already admitted; the workspace decides whether to
//! answer, from its own store.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result};
use iroh::{
    Endpoint, PublicKey,
    endpoint::{Connection, RecvStream, SendStream, VarInt},
    protocol::{AcceptError, ProtocolHandler},
};
use tokio::sync::{mpsc, oneshot};
use tracing::warn;

use super::gossip::Incoming;
use crate::{
    ops::MAX_OP_BYTES,
    sync::key_handoff::{KeyAnswer, KeyRequest, MAX_HANDOFF_COMMIT_BYTES, MAX_HANDOFF_COMMITS},
};

pub const KEY_HANDOFF_ALPN: &[u8] = b"shyspace/storage-key/1";
/// Two numbers; anything longer is refused unread.
const MAX_REQUEST_BYTES: usize = 32;
/// The commits and the settings op an answer may carry, with room for
/// their framing.
const MAX_ANSWER_BYTES: usize = MAX_HANDOFF_COMMIT_BYTES + MAX_OP_BYTES + MAX_HANDOFF_COMMITS * 16 + 1024;
/// How long an asker waits for the whole answer.
const ASK_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a request may take to arrive, and the workspace to answer it.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);
/// Requests come only from members whose key stopped working, rarely; a
/// few at once is plenty, and one per device.
const MAX_OPEN: usize = 8;
pub(super) const REFUSED: VarInt = VarInt::from_u32(2);
const ANSWERED: VarInt = VarInt::from_u32(1);

/// A request for the workspace, with where its answer goes; `None` means
/// it isn't answered.
pub struct KeyRequestIn {
    pub from: PublicKey,
    pub request: KeyRequest,
    pub reply: oneshot::Sender<Option<KeyAnswer>>,
}

impl std::fmt::Debug for KeyRequestIn {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KeyRequestIn")
            .field("from", &self.from)
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct KeyHandoff {
    incoming: mpsc::Sender<Incoming>,
    open: Arc<Mutex<HashSet<PublicKey>>>,
}

/// Holds a device's one open connection until dropped.
struct OpenSlot {
    open: Arc<Mutex<HashSet<PublicKey>>>,
    from: PublicKey,
}

impl Drop for OpenSlot {
    fn drop(&mut self) {
        let mut open = self.open.lock().expect("key handoff counts lock");
        open.remove(&self.from);
    }
}

impl KeyHandoff {
    pub fn new(incoming: mpsc::Sender<Incoming>) -> Self {
        Self {
            incoming,
            open: Arc::default(),
        }
    }

    fn open_slot(&self, from: PublicKey) -> Option<OpenSlot> {
        let mut open = self.open.lock().expect("key handoff counts lock");
        if open.len() >= MAX_OPEN || !open.insert(from) {
            return None;
        }
        Some(OpenSlot {
            open: self.open.clone(),
            from,
        })
    }

    /// Reads the request, has the workspace decide, and sends its answer.
    async fn answer(
        &self,
        from: PublicKey,
        send: &mut SendStream,
        receive: &mut RecvStream,
    ) -> Result<Option<KeyAnswer>> {
        let bytes = receive.read_to_end(MAX_REQUEST_BYTES).await?;
        let request: KeyRequest = postcard::from_bytes(&bytes)?;
        let (reply, answer) = oneshot::channel();
        self.incoming
            .try_send(Incoming::KeyRequest(KeyRequestIn { from, request, reply }))
            .context("the workspace is busy")?;
        let Some(answer) = answer.await? else {
            return Ok(None);
        };
        send.write_all(&postcard::to_stdvec(&answer)?).await?;
        send.finish()?;
        Ok(Some(answer))
    }
}

impl ProtocolHandler for KeyHandoff {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Some(_slot) = self.open_slot(connection.remote_id()) else {
            connection.close(REFUSED, b"busy");
            return Ok(());
        };
        let deadline = tokio::time::Instant::now() + ANSWER_TIMEOUT;
        let Ok(Ok((mut send, mut receive))) = tokio::time::timeout_at(deadline, connection.accept_bi()).await
        else {
            connection.close(REFUSED, b"no request");
            return Ok(());
        };
        let answering = self.answer(connection.remote_id(), &mut send, &mut receive);
        let reason: &[u8] = match tokio::time::timeout_at(deadline, answering).await {
            Ok(Ok(Some(_))) => {
                // Let the answer reach the asker before closing.
                let _ = tokio::time::timeout(ANSWER_TIMEOUT, connection.closed()).await;
                connection.close(ANSWERED, b"answered");
                return Ok(());
            }
            Ok(Ok(None)) => b"not answered",
            Ok(Err(error)) => {
                warn!("couldn't answer a request for the storage settings: {error:#}");
                b"failed"
            }
            Err(_) => b"too slow",
        };
        // Dropping an unfinished stream ends it cleanly, which can reach the
        // asker before the close and read as an empty answer.
        let _ = send.reset(REFUSED);
        connection.close(REFUSED, reason);
        Ok(())
    }
}

/// Asks `member`'s device for the storage settings and reads its answer.
pub async fn ask_for_storage_key(
    endpoint: &Endpoint,
    member: PublicKey,
    request: &KeyRequest,
) -> Result<KeyAnswer> {
    let bytes = postcard::to_stdvec(request)?;
    tokio::time::timeout(ASK_TIMEOUT, async {
        let connection = endpoint.connect(member, KEY_HANDOFF_ALPN).await?;
        let (mut send, mut receive) = connection.open_bi().await?;
        send.write_all(&bytes).await?;
        send.finish()?;
        let answer = receive.read_to_end(MAX_ANSWER_BYTES).await.context("no answer")?;
        connection.close(ANSWERED, b"read");
        Ok(postcard::from_bytes(&answer)?)
    })
    .await
    .context("asking for the storage settings took too long")?
}
