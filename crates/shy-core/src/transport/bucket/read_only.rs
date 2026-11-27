//! A bucket that can only be read. A device that is only linking, reading
//! with keys its person's device handed it, may write nothing at all; its
//! store refuses every put, delete, copy and upload, and its bucket has no
//! signer, so it can't presign either. This is the guarantee; the friendly
//! "can send once added" messages sit in front of it.

use std::sync::Arc;

use object_store::{ObjectStore, path::Path};

use super::{Bucket, Connector};

const REFUSAL: &str = "a device that is only linking writes nothing to the bucket";

/// The only bucket a reading workspace can be built with.
pub struct ReadingBucket(Bucket);

impl ReadingBucket {
    pub fn over(bucket: Bucket) -> Self {
        Self(Bucket {
            store: Arc::new(ReadOnlyStore(bucket.store)),
            signing: None,
        })
    }

    pub(crate) fn into_bucket(self) -> Bucket {
        self.0
    }
}

/// Connects only reading buckets, so a reconnect after the bucket keys
/// change can't give a reading workspace a writable one.
#[derive(Clone)]
pub struct ReadingConnector(Connector);

impl ReadingConnector {
    pub fn over(connect: Connector) -> Self {
        Self(connect)
    }

    pub fn connect(&self, workspace_id: &str, config: &super::BucketConfig) -> anyhow::Result<ReadingBucket> {
        Ok(ReadingBucket::over((self.0)(workspace_id, config)?))
    }

    pub(crate) fn into_connector(self) -> Connector {
        Arc::new(move |workspace_id, config| Ok(self.connect(workspace_id, config)?.into_bucket()))
    }
}

#[derive(Debug)]
struct ReadOnlyStore(Arc<dyn ObjectStore>);

impl std::fmt::Display for ReadOnlyStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "read-only {}", self.0)
    }
}

fn refused() -> object_store::Error {
    object_store::Error::NotSupported {
        source: REFUSAL.into(),
    }
}

/// What an `async fn` in the trait comes to: `object_store` declares it with
/// `async_trait`, which this crate doesn't depend on outside tests.
type Boxed<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = object_store::Result<T>> + Send + 'a>>;

impl ObjectStore for ReadOnlyStore {
    fn put_opts<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _location: &'life1 Path,
        _payload: object_store::PutPayload,
        _opts: object_store::PutOptions,
    ) -> Boxed<'async_trait, object_store::PutResult>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Err(refused()) })
    }

    fn put_multipart_opts<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _location: &'life1 Path,
        _opts: object_store::PutMultipartOptions,
    ) -> Boxed<'async_trait, Box<dyn object_store::MultipartUpload>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Err(refused()) })
    }

    fn get_opts<'life0, 'life1, 'async_trait>(
        &'life0 self,
        location: &'life1 Path,
        options: object_store::GetOptions,
    ) -> Boxed<'async_trait, object_store::GetResult>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        self.0.get_opts(location, options)
    }

    fn delete_stream(
        &self,
        locations: futures_lite::stream::Boxed<object_store::Result<Path>>,
    ) -> futures_lite::stream::Boxed<object_store::Result<Path>> {
        Box::pin(futures_lite::StreamExt::map(locations, |_| Err(refused())))
    }

    fn list(
        &self,
        prefix: Option<&Path>,
    ) -> futures_lite::stream::Boxed<object_store::Result<object_store::ObjectMeta>> {
        self.0.list(prefix)
    }

    fn list_with_offset(
        &self,
        prefix: Option<&Path>,
        offset: &Path,
    ) -> futures_lite::stream::Boxed<object_store::Result<object_store::ObjectMeta>> {
        self.0.list_with_offset(prefix, offset)
    }

    fn list_with_delimiter<'life0, 'life1, 'async_trait>(
        &'life0 self,
        prefix: Option<&'life1 Path>,
    ) -> Boxed<'async_trait, object_store::ListResult>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        self.0.list_with_delimiter(prefix)
    }

    fn copy_opts<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        _from: &'life1 Path,
        _to: &'life2 Path,
        _options: object_store::CopyOptions,
    ) -> Boxed<'async_trait, ()>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Err(refused()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading_over_memory() -> (Bucket, Bucket) {
        let writable = Bucket::without_signing(Arc::new(object_store::memory::InMemory::new()));
        let reading = ReadingBucket::over(writable.clone()).into_bucket();
        (writable, reading)
    }

    #[tokio::test]
    async fn a_reading_bucket_reads_what_is_there_and_writes_nothing() {
        let (writable, reading) = reading_over_memory();
        let token = [1; 16];
        writable
            .put_link_slot(&token, "answer", b"handed".to_vec())
            .await
            .unwrap();

        assert_eq!(
            reading
                .link_slot(&token, "answer")
                .await
                .unwrap()
                .unwrap()
                .as_ref(),
            b"handed"
        );
        assert!(
            reading
                .put_link_slot(&token, "request", b"x".to_vec())
                .await
                .is_err()
        );
        assert!(reading.delete_link_slot(&token, "answer").await.is_err());
        assert!(
            writable.link_slot(&token, "answer").await.unwrap().is_some(),
            "nothing deleted"
        );
        assert!(
            writable.link_slot(&token, "request").await.unwrap().is_none(),
            "nothing written"
        );
    }

    #[tokio::test]
    async fn a_reading_bucket_presigns_nothing() {
        let (_, reading) = reading_over_memory();

        assert!(
            reading
                .presign_link(
                    &[1; 16],
                    std::time::Duration::from_secs(60),
                    std::time::Duration::from_secs(60)
                )
                .await
                .is_err()
        );
    }
}
