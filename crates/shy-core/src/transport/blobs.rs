//! Shared files' encrypted pieces in the bucket, under the author's own
//! prefix: `files/<author>/<blob id>/<piece>`. Pieces are checked against the
//! hashes their author signed, so this layer only bounds their size.

use anyhow::{Result, ensure};
use bytes::Bytes;
use iroh::PublicKey;
use object_store::path::Path;

use super::bucket::Bucket;
use crate::{files::blob::is_blob_id, views::member_id};

fn blob_prefix(author: &PublicKey, blob_id: &str) -> Result<String> {
    ensure!(is_blob_id(blob_id), "malformed file id");
    Ok(format!("files/{}/{blob_id}", member_id(author)))
}

fn chunk_path(author: &PublicKey, blob_id: &str, index: u32) -> Result<Path> {
    Ok(Path::from(format!(
        "{}/{index:010}",
        blob_prefix(author, blob_id)?
    )))
}

impl Bucket {
    pub async fn put_file_chunk(
        &self,
        author: &PublicKey,
        blob_id: &str,
        index: u32,
        sealed: Vec<u8>,
    ) -> Result<()> {
        self.put(&chunk_path(author, blob_id, index)?, sealed).await
    }

    /// A piece, if it's there and no larger than it should be.
    pub async fn file_chunk(
        &self,
        author: &PublicKey,
        blob_id: &str,
        index: u32,
        expected_len: u64,
    ) -> Result<Option<Bytes>> {
        self.get_at_most(&chunk_path(author, blob_id, index)?, expected_len)
            .await
    }

    /// Removes every piece of a blob. Removing what's already gone is fine.
    pub async fn delete_blob(&self, author: &PublicKey, blob_id: &str) -> Result<()> {
        for path in self.list(&blob_prefix(author, blob_id)?, None).await? {
            self.delete(&path).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
impl Bucket {
    /// The ids of every blob `author` has pieces of.
    pub async fn blob_ids(&self, author: &PublicKey) -> Result<Vec<String>> {
        let mut ids: Vec<String> = self
            .list(&format!("files/{}", member_id(author)), None)
            .await?
            .iter()
            .filter_map(|path| path.parts().nth(2).map(|part| part.as_ref().to_owned()))
            .collect();
        ids.dedup();
        Ok(ids)
    }
}
