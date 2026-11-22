//! A channel written to straight through the store, for storage tests.

use iroh::{PublicKey, SecretKey};

use super::{Store, TEST_DB_KEY};
use crate::ops::{Op, OpBody, VerifiedOp, new_owned_id};

/// One channel, written to straight through the store.
pub(crate) struct TestChannel {
    pub store: Store,
    pub author: PublicKey,
    pub id: String,
    seq: u64,
    _dir: tempfile::TempDir,
}

impl TestChannel {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();
        let author = SecretKey::generate().public();
        let id = new_owned_id(&author);
        let mut channel = TestChannel {
            store,
            author,
            id: id.clone(),
            seq: 0,
            _dir: dir,
        };
        channel.apply(Op::ChannelCreate {
            id,
            name: "general".into(),
        });
        channel
    }

    pub fn apply(&mut self, op: Op) {
        self.seq += 1;
        self.apply_again(op);
    }

    /// Applies `op` as the same op as the last one applied, as when it
    /// arrives a second time.
    pub fn apply_again(&mut self, op: Op) {
        self.store
            .apply(&VerifiedOp {
                author: self.author,
                seq: self.seq,
                epoch: 1,
                body: OpBody {
                    lamport: self.seq,
                    timestamp_ms: self.seq as i64,
                    op,
                },
            })
            .unwrap();
    }

    pub fn post(&mut self, text: &str, reply_to: Option<&str>) -> String {
        let id = new_owned_id(&self.author);
        self.apply(Op::Message {
            id: id.clone(),
            channel: self.id.clone(),
            text: text.into(),
            reply_to: reply_to.map(str::to_owned),
        });
        id
    }

    /// `count` threads, each a first message with a reply and a reply to
    /// that reply. Returns the first messages.
    pub fn post_threads(&mut self, count: usize) -> Vec<String> {
        (0..count)
            .map(|_| {
                let root = self.post("first", None);
                let reply = self.post("reply", Some(&root));
                self.post("reply to the reply", Some(&reply));
                root
            })
            .collect()
    }
}
