//! Small messages straight to one member, for things only some members may
//! know, like a private channel's huddle. Each message is one connection
//! that the members-only hooks already admitted, so the receiver learns for
//! certain which device sent it, and the sender learns it was taken.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use iroh::{
    Endpoint, PublicKey,
    endpoint::{Connection, ConnectionError, VarInt},
    protocol::{AcceptError, ProtocolHandler},
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::warn;

use super::gossip::Incoming;
use crate::huddle::beacon::{Beacon, MAX_BEACON_BYTES};

pub const DIRECT_ALPN: &[u8] = b"shyspace/direct/1";
/// A message larger than this is refused unread.
const MAX_MESSAGE_BYTES: usize = MAX_BEACON_BYTES + 64;
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
/// A sender that doesn't finish its message within this is cut off.
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(5);
/// How the receiver closes: the message was taken, or it wasn't.
/// Direct messages read at once. Members send a few per minute each, so
/// more than this means someone is flooding; the rest are refused.
const MAX_OPEN_CONNECTIONS: usize = 64;
/// One device sends one message at a time to each member, so a few at once
/// is plenty; more is someone hogging the shared limit.
const MAX_OPEN_PER_DEVICE: usize = 4;
const READ: VarInt = VarInt::from_u32(1);
const REFUSED: VarInt = VarInt::from_u32(2);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DirectMessage {
    Huddle(Beacon),
}

/// Hands each arriving message to the workspace, with the sending device.
#[derive(Debug, Clone)]
pub struct Direct {
    incoming: mpsc::Sender<Incoming>,
    open: Arc<Mutex<OpenCounts>>,
}

/// Direct connections being read, in all and per sending device.
#[derive(Debug, Default)]
struct OpenCounts {
    total: usize,
    per_device: HashMap<PublicKey, usize>,
}

/// Counts one open connection until dropped.
struct OpenSlot {
    open: Arc<Mutex<OpenCounts>>,
    from: PublicKey,
}

impl Drop for OpenSlot {
    fn drop(&mut self) {
        let mut counts = self.open.lock().expect("direct connection counts lock");
        counts.total -= 1;
        if let Some(count) = counts.per_device.get_mut(&self.from) {
            *count -= 1;
            if *count == 0 {
                counts.per_device.remove(&self.from);
            }
        }
    }
}

impl Direct {
    pub fn new(incoming: mpsc::Sender<Incoming>) -> Self {
        Self {
            incoming,
            open: Arc::default(),
        }
    }

    /// A slot for one more connection from `from`, unless it or everyone
    /// together already has too many open.
    fn open_slot(&self, from: PublicKey) -> Option<OpenSlot> {
        let mut counts = self.open.lock().expect("direct connection counts lock");
        let from_device = counts.per_device.get(&from).copied().unwrap_or(0);
        if counts.total >= MAX_OPEN_CONNECTIONS || from_device >= MAX_OPEN_PER_DEVICE {
            return None;
        }
        counts.total += 1;
        *counts.per_device.entry(from).or_default() += 1;
        Some(OpenSlot {
            open: self.open.clone(),
            from,
        })
    }
}

impl ProtocolHandler for Direct {
    /// One message per connection: read it within the time limit, hand it
    /// over, then close with `READ` so the sender knows it arrived.
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let from = connection.remote_id();
        let Some(_slot) = self.open_slot(from) else {
            connection.close(REFUSED, b"busy");
            return Ok(());
        };
        let read = tokio::time::timeout(RECEIVE_TIMEOUT, async {
            let mut stream = connection.accept_uni().await.map_err(AcceptError::from_err)?;
            stream
                .read_to_end(MAX_MESSAGE_BYTES)
                .await
                .map_err(AcceptError::from_err)
        })
        .await;
        let bytes = match read {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(error)) => {
                connection.close(REFUSED, b"unreadable");
                return Err(error);
            }
            Err(_) => {
                connection.close(REFUSED, b"too slow");
                return Ok(());
            }
        };
        let message = match postcard::from_bytes::<DirectMessage>(&bytes) {
            Ok(message) => message,
            Err(error) => {
                connection.close(REFUSED, b"malformed");
                return Err(AcceptError::from_err(error));
            }
        };
        // A full queue drops the message rather than holding up gossip;
        // beacons are re-sent every refresh.
        match self.incoming.try_send(Incoming::Direct { from, message }) {
            Ok(()) => connection.close(READ, b"read"),
            Err(error) => {
                warn!("dropping a direct message: {error}");
                connection.close(REFUSED, b"busy");
            }
        }
        Ok(())
    }
}

/// Sends `message` to `to`'s device and waits until its app has taken it.
pub async fn send_direct(endpoint: &Endpoint, to: PublicKey, message: &DirectMessage) -> Result<()> {
    let bytes = postcard::to_stdvec(message)?;
    let closed = tokio::time::timeout(SEND_TIMEOUT, async {
        let connection = endpoint.connect(to, DIRECT_ALPN).await?;
        let mut stream = connection.open_uni().await?;
        stream.write_all(&bytes).await?;
        stream.finish()?;
        anyhow::Ok(connection.closed().await)
    })
    .await
    .context("a direct message took too long")??;
    match closed {
        ConnectionError::ApplicationClosed(close) if close.error_code == READ => Ok(()),
        other => bail!("a direct message wasn't taken: {other}"),
    }
}
