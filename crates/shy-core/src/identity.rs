//! A member's identity: one ed25519 key that is their network address, signs
//! their ops, and names them in the MLS group. It lives in the encrypted store.

use anyhow::Result;
use iroh::SecretKey;
use zeroize::Zeroizing;

use crate::storage::Store;

const IDENTITY_META: &str = "identity_key";

pub fn load_or_create(store: &Store) -> Result<SecretKey> {
    if let Some(bytes) = store.meta(IDENTITY_META)? {
        let bytes = Zeroizing::new(bytes);
        let key: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("the stored identity key is corrupt"))?;
        return Ok(SecretKey::from_bytes(&key));
    }
    let identity = SecretKey::generate();
    store.set_meta(IDENTITY_META, Zeroizing::new(identity.to_bytes()).as_slice())?;
    Ok(identity)
}

/// Makes `identity` this store's identity: a new device's own, in the store
/// it reads from before it is added, or a key a test chose.
pub fn store(store: &Store, identity: &SecretKey) -> Result<()> {
    store.set_meta(IDENTITY_META, Zeroizing::new(identity.to_bytes()).as_slice())
}
