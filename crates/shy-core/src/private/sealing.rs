//! This member's sealing key, which others wrap channel keys to.

use anyhow::Result;

use super::crypto::new_sealing_key_pair;
use crate::{ops::Op, workspace::Workspace};

impl Workspace {
    /// Makes sure others can hand us channel keys: the newest sealing key is
    /// announced, and the first one is made on first start.
    pub(crate) fn announce_sealing_key(&self) -> Result<()> {
        let (newest, announced) = {
            let state = self.state();
            if !state.mls.is_active() {
                return Ok(());
            }
            let newest = match state.store.sealing_secrets()?.into_iter().next() {
                Some(pair) => pair.public,
                None => {
                    let pair = new_sealing_key_pair()?;
                    state.store.put_sealing_secret(&pair)?;
                    pair.public
                }
            };
            (newest, state.store.sealing_key_of(&self.me())?)
        };
        if announced.as_deref() == Some(newest.as_slice()) {
            return Ok(());
        }
        self.write(Op::SealingKey { public: newest })
    }
}
