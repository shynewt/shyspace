//! Builds a demo workspace for trying the app by hand: a few people, channels,
//! threads, a private channel, DMs, pages, statuses and reminders, on a real
//! bucket. Every person gets a data directory under the given root, so the app
//! (`SHY_DIR=<root>/ada`) or the CLI can open any of them afterwards.

mod cast;
mod pages;
mod story;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use shy_core::BucketConfig;

/// Left in the root so a later run knows it may delete it.
const MARKER: &str = ".shy-demo";

#[tokio::main]
async fn main() -> Result<()> {
    let root: PathBuf = std::env::args_os()
        .nth(1)
        .context("usage: shy-demo <directory>")?
        .into();
    prepare(&root)?;
    let cast = cast::assemble(&root, bucket_from_env()).await?;
    story::tell(&cast).await?;
    cast.shut_down().await?;
    println!("{}", root.join(cast::ADMIN.to_lowercase()).display());
    Ok(())
}

/// Starts `root` empty, deleting an earlier demo there but nothing else.
fn prepare(root: &Path) -> Result<()> {
    if root.exists() {
        ensure!(
            root.join(MARKER).exists(),
            "{} exists and isn't a demo; pick another directory",
            root.display()
        );
        std::fs::remove_dir_all(root)
            .with_context(|| format!("deleting the old demo in {}", root.display()))?;
    }
    std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
    std::fs::write(root.join(MARKER), "").context("marking the demo directory")?;
    Ok(())
}

/// The bucket from the `SHY_S3_*` variables the CLI reads, else the local one in docs/development.md.
fn bucket_from_env() -> BucketConfig {
    let setting = |name: &str, local: &str| std::env::var(name).unwrap_or_else(|_| local.to_owned());
    BucketConfig {
        endpoint: setting("SHY_S3_ENDPOINT", "http://localhost:9000"),
        region: setting("SHY_S3_REGION", "us-east-1"),
        bucket: setting("SHY_S3_BUCKET", "shyspace"),
        access_key_id: setting("SHY_S3_ACCESS_KEY_ID", "shyadmin"),
        secret_access_key: setting("SHY_S3_SECRET_ACCESS_KEY", "shysecret123"),
    }
}

/// Fails with what went wrong if `member` couldn't sync.
fn check_synced(name: &str, member: &shy_core::Workspace) -> Result<()> {
    match member.status().error {
        Some(error) => bail!("{name} could not sync: {error}"),
        None => Ok(()),
    }
}
