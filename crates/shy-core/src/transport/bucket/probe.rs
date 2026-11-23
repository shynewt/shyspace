//! Checks a real bucket provider does what sync relies on: listings carry
//! an ETag that matches the upload's, an overwrite changes it at once, a new
//! object is listed straight after it's written, listing after an offset
//! skips exactly the segment names that hold nothing more, and a create-only
//! write (If-None-Match) is refused over an existing object. Ignored by default;
//! run it against a bucket you may write to:
//!
//! ```sh
//! SHY_BUCKET_PROBE=/path/to/probe.env cargo test -p shy-core --lib \
//!     bucket_probe -- --ignored --nocapture
//! ```
//!
//! The file holds `SHY_S3_ENDPOINT`, `SHY_S3_REGION`, `SHY_S3_BUCKET`,
//! `SHY_S3_ACCESS_KEY_ID` and `SHY_S3_SECRET_ACCESS_KEY`, one `NAME=value` per
//! line. Nothing from it is printed. Everything is written under
//! `probe/<random>/` and deleted afterwards, also when a check fails.

use std::collections::HashMap;

use anyhow::{Context, Result, ensure};
use futures_lite::StreamExt;
use object_store::{ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, path::Path};

use super::{BucketConfig, listing_after, object_version, segment_path};

pub(crate) fn read_settings(file: &str) -> Result<BucketConfig> {
    let text = std::fs::read_to_string(file).context("reading the probe settings file")?;
    let values: HashMap<&str, &str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| (name.trim(), value.trim().trim_matches('"')))
        .collect();
    let value = |name: &str| -> Result<String> {
        values
            .get(name)
            .map(|value| (*value).to_owned())
            .with_context(|| format!("{name} is missing from the probe settings file"))
    };
    Ok(BucketConfig {
        endpoint: value("SHY_S3_ENDPOINT")?,
        region: value("SHY_S3_REGION")?,
        bucket: value("SHY_S3_BUCKET")?,
        access_key_id: value("SHY_S3_ACCESS_KEY_ID")?,
        secret_access_key: value("SHY_S3_SECRET_ACCESS_KEY")?,
    })
}

async fn listed_versions(store: &dyn ObjectStore) -> Result<HashMap<String, Option<String>>> {
    let mut listing = store.list(None);
    let mut versions = HashMap::new();
    while let Some(meta) = listing.next().await {
        let meta = meta?;
        versions.insert(meta.location.to_string(), meta.e_tag);
    }
    Ok(versions)
}

async fn run_checks(store: &dyn ObjectStore) -> Result<()> {
    let head = Path::from("heads/probe");
    let first = store.put(&head, PutPayload::from(vec![1u8; 200])).await?;
    let listed = listed_versions(store).await?;
    ensure!(
        listed.contains_key(head.as_ref()),
        "a new object isn't listed straight after it's written"
    );
    let listed_tag = listed[head.as_ref()].clone();
    ensure!(listed_tag.is_some(), "listings carry no ETag");
    ensure!(
        listed_tag == first.e_tag,
        "the listed ETag differs from the upload's"
    );
    println!("ok  a new object is listed at once, with the upload's ETag");

    store.put(&head, PutPayload::from(vec![2u8; 200])).await?;
    let changed = listed_versions(store).await?[head.as_ref()].clone();
    ensure!(
        changed != listed_tag,
        "overwriting with other bytes of the same size kept the ETag"
    );
    let fetched = store.get(&head).await?.meta;
    ensure!(
        fetched.e_tag == changed,
        "a fetch shows another ETag than the listing"
    );
    println!("ok  an overwrite changes the ETag at once, and a fetch agrees");

    let author = iroh::SecretKey::generate().public();
    let holds_up_to_5 = segment_path(&author, 1, 5, &[1; 16], b"one");
    let holds_more = segment_path(&author, 6, 7, &[2; 16], b"two");
    for path in [&holds_up_to_5, &holds_more] {
        store.put(path, PutPayload::from(b"segment".to_vec())).await?;
    }
    let prefix = Path::from(format!("ops/{}", crate::views::member_id(&author)));
    let offset = Path::from(listing_after(&author, 5));
    let mut after = store.list_with_offset(Some(&prefix), &offset);
    let mut found = Vec::new();
    while let Some(meta) = after.next().await {
        let meta = meta?;
        found.push((meta.location.to_string(), object_version(&meta)));
    }
    let names: Vec<_> = found.iter().map(|(name, _)| name.as_str()).collect();
    ensure!(
        names == [holds_more.as_ref()],
        "listing after what is held returned {} names instead of exactly the newer one",
        names.len()
    );
    println!("ok  listing after what is held skips exactly the names holding nothing more");

    let written_once = Path::from("ops/create-only");
    let create = || PutOptions {
        mode: PutMode::Create,
        ..PutOptions::default()
    };
    store
        .put_opts(&written_once, PutPayload::from(b"first".to_vec()), create())
        .await
        .context("a create-only write of a new object failed")?;
    match store
        .put_opts(&written_once, PutPayload::from(b"second".to_vec()), create())
        .await
    {
        Err(object_store::Error::AlreadyExists { .. }) => {}
        Ok(_) => anyhow::bail!("a create-only write replaced an existing object"),
        Err(error) => anyhow::bail!("a create-only write over an existing object failed oddly: {error}"),
    }
    let kept = store.get(&written_once).await?.bytes().await?;
    ensure!(kept.as_ref() == b"first", "the existing object changed");
    println!("ok  a create-only write is refused over an existing object, which stays as it was");
    Ok(())
}

async fn delete_everything(store: &dyn ObjectStore) -> Result<()> {
    for path in listed_versions(store).await?.into_keys() {
        store.delete(&Path::from(path)).await?;
    }
    ensure!(
        listed_versions(store).await?.is_empty(),
        "objects were left behind"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "talks to a real bucket; set SHY_BUCKET_PROBE"]
async fn bucket_probe() {
    let file = std::env::var("SHY_BUCKET_PROBE").expect("set SHY_BUCKET_PROBE to the settings file");
    let settings = read_settings(&file).unwrap();
    let scratch = format!("probe/{}", crate::ops::new_random_id());
    let bucket = settings.connect(&scratch).unwrap();
    let checked = run_checks(bucket.store.as_ref()).await;
    let cleaned = delete_everything(bucket.store.as_ref()).await;
    if cleaned.is_ok() {
        println!("ok  everything the probe wrote is gone");
    }
    checked.unwrap();
    cleaned.unwrap();
}
