//! An in-memory bucket that counts requests, to turn sync behaviour into
//! request counts (and so into a provider's bill).

use std::{
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use futures_lite::{StreamExt, stream};
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, memory::InMemory, path::Path,
};

use crate::transport::bucket::Bucket;

/// S3 returns at most this many keys per list request.
const KEYS_PER_LIST_PAGE: u64 = 1000;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Requests {
    pub lists: u64,
    /// Keys returned by all lists, to estimate how many pages S3 would send.
    pub listed_keys: u64,
    pub gets: u64,
    pub puts: u64,
    pub deletes: u64,
}

impl Requests {
    /// List requests as S3 would bill them: at least one page per list.
    pub fn list_pages(&self) -> u64 {
        self.lists + self.listed_keys / KEYS_PER_LIST_PAGE
    }

    pub fn since(&self, earlier: &Requests) -> Requests {
        Requests {
            lists: self.lists - earlier.lists,
            listed_keys: self.listed_keys - earlier.listed_keys,
            gets: self.gets - earlier.gets,
            puts: self.puts - earlier.puts,
            deletes: self.deletes - earlier.deletes,
        }
    }
}

impl fmt::Display for Requests {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} list pages, {} gets, {} puts, {} deletes",
            self.list_pages(),
            self.gets,
            self.puts,
            self.deletes
        )
    }
}

#[derive(Debug, Default)]
struct Counters {
    lists: AtomicU64,
    listed_keys: AtomicU64,
    gets: AtomicU64,
    puts: AtomicU64,
    deletes: AtomicU64,
}

#[derive(Debug)]
pub struct CountingStore {
    inner: Arc<dyn ObjectStore>,
    counters: Arc<Counters>,
    fetched_keys: Mutex<Vec<String>>,
    listed_prefixes: Mutex<Vec<String>>,
    /// Added to every request while set, to act like a bucket over the internet.
    delay_ms: AtomicU64,
}

impl Default for CountingStore {
    fn default() -> Self {
        Self {
            inner: Arc::new(InMemory::new()),
            counters: Arc::default(),
            fetched_keys: Mutex::default(),
            listed_prefixes: Mutex::default(),
            delay_ms: AtomicU64::default(),
        }
    }
}

impl CountingStore {
    /// Counts requests to `inner` into the same totals as this store's.
    pub fn wrapping(&self, inner: Arc<dyn ObjectStore>) -> Self {
        Self {
            inner,
            counters: self.counters.clone(),
            fetched_keys: Mutex::default(),
            listed_prefixes: Mutex::default(),
            delay_ms: AtomicU64::default(),
        }
    }

    pub fn requests(&self) -> Requests {
        let counters = &self.counters;
        Requests {
            lists: counters.lists.load(Ordering::SeqCst),
            listed_keys: counters.listed_keys.load(Ordering::SeqCst),
            gets: counters.gets.load(Ordering::SeqCst),
            puts: counters.puts.load(Ordering::SeqCst),
            deletes: counters.deletes.load(Ordering::SeqCst),
        }
    }

    /// The keys of every object downloaded so far, in order.
    pub fn fetched_keys(&self) -> Vec<String> {
        self.fetched_keys.lock().unwrap().clone()
    }

    /// How many listings were made under exactly `prefix`.
    pub fn listings_of(&self, prefix: &str) -> usize {
        let listed = self.listed_prefixes.lock().unwrap();
        listed.iter().filter(|listed| listed.as_str() == prefix).count()
    }

    fn note_listing(&self, prefix: Option<&Path>) {
        let prefix = prefix.map(Path::to_string).unwrap_or_default();
        self.listed_prefixes.lock().unwrap().push(prefix);
    }

    pub fn set_delay(&self, delay: Duration) {
        let delay_ms = u64::try_from(delay.as_millis()).unwrap();
        self.delay_ms.store(delay_ms, Ordering::SeqCst);
    }

    fn delay(&self) -> Duration {
        Duration::from_millis(self.delay_ms.load(Ordering::SeqCst))
    }

    async fn wait(&self) {
        tokio::time::sleep(self.delay()).await;
    }

    fn counted_list(
        &self,
        listing: stream::Boxed<object_store::Result<ObjectMeta>>,
    ) -> stream::Boxed<object_store::Result<ObjectMeta>> {
        self.counters.lists.fetch_add(1, Ordering::SeqCst);
        let counters = self.counters.clone();
        let counted = listing.inspect(move |_| {
            counters.listed_keys.fetch_add(1, Ordering::SeqCst);
        });
        let mut listing = Some(counted);
        Box::pin(
            stream::once_future(tokio::time::sleep(self.delay()))
                .flat_map(move |()| listing.take().expect("a list waits once")),
        )
    }
}

/// A bucket for members to share, and the store behind it to read counts from.
pub fn counting_bucket() -> (Arc<CountingStore>, Bucket) {
    let store = Arc::new(CountingStore::default());
    let bucket = Bucket::without_signing(store.clone());
    (store, bucket)
}

impl fmt::Display for CountingStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("counting test store")
    }
}

#[async_trait]
impl ObjectStore for CountingStore {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        opts: PutOptions,
    ) -> object_store::Result<PutResult> {
        self.counters.puts.fetch_add(1, Ordering::SeqCst);
        self.wait().await;
        self.inner.put_opts(location, payload, opts).await
    }

    async fn put_multipart_opts(
        &self,
        location: &Path,
        opts: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.counters.puts.fetch_add(1, Ordering::SeqCst);
        self.inner.put_multipart_opts(location, opts).await
    }

    async fn get_opts(&self, location: &Path, options: GetOptions) -> object_store::Result<GetResult> {
        self.counters.gets.fetch_add(1, Ordering::SeqCst);
        self.fetched_keys.lock().unwrap().push(location.to_string());
        self.wait().await;
        self.inner.get_opts(location, options).await
    }

    fn delete_stream(
        &self,
        locations: stream::Boxed<object_store::Result<Path>>,
    ) -> stream::Boxed<object_store::Result<Path>> {
        let counters = self.counters.clone();
        let counted = Box::pin(locations.inspect(move |_| {
            counters.deletes.fetch_add(1, Ordering::SeqCst);
        }));
        self.inner.delete_stream(counted)
    }

    fn list(&self, prefix: Option<&Path>) -> stream::Boxed<object_store::Result<ObjectMeta>> {
        self.note_listing(prefix);
        self.counted_list(self.inner.list(prefix))
    }

    fn list_with_offset(
        &self,
        prefix: Option<&Path>,
        offset: &Path,
    ) -> stream::Boxed<object_store::Result<ObjectMeta>> {
        self.note_listing(prefix);
        self.counted_list(self.inner.list_with_offset(prefix, offset))
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        self.counters.lists.fetch_add(1, Ordering::SeqCst);
        self.wait().await;
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy_opts(&self, from: &Path, to: &Path, options: CopyOptions) -> object_store::Result<()> {
        self.counters.puts.fetch_add(1, Ordering::SeqCst);
        self.inner.copy_opts(from, to, options).await
    }
}
