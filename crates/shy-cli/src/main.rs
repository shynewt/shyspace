mod mcp;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use shy_core::{BucketConfig, Networking, Profile, Workspace};

#[derive(Parser)]
#[command(
    name = "shy",
    about = "Headless shyspace member: join workspaces and serve them to AI agents over MCP"
)]
struct Cli {
    /// Where this member's keys and data live.
    #[arg(long, env = "SHY_DIR", global = true)]
    dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new workspace and become its admin.
    Create {
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        name: String,
        #[command(flatten)]
        bucket: BucketArgs,
    },
    /// Join a workspace with an invite code. Waits until the admin's app accepts.
    Join {
        invite: String,
        #[arg(long)]
        name: String,
        /// Mark this member as a bot in everyone's member list.
        #[arg(long)]
        bot: bool,
    },
    /// Print an invite code (admin only).
    Invite,
    /// Serve the workspace to an AI agent over MCP on stdin/stdout.
    Mcp,
}

#[derive(clap::Args)]
struct BucketArgs {
    #[arg(long, env = "SHY_S3_ENDPOINT")]
    endpoint: String,
    #[arg(long, env = "SHY_S3_REGION", default_value = "us-east-1")]
    region: String,
    #[arg(long, env = "SHY_S3_BUCKET")]
    bucket: String,
    #[arg(long, env = "SHY_S3_ACCESS_KEY_ID")]
    access_key_id: String,
    #[arg(long, env = "SHY_S3_SECRET_ACCESS_KEY")]
    secret_access_key: String,
}

fn default_dir() -> Result<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .context("cannot find a data directory; pass --dir")?;
    Ok(data_home.join("shyspace/cli"))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    let cli = Cli::parse();
    let dir = match cli.dir {
        Some(dir) => dir,
        None => default_dir()?,
    };
    match cli.command {
        Command::Create {
            workspace,
            name,
            bucket,
        } => {
            let config = BucketConfig {
                endpoint: bucket.endpoint,
                region: bucket.region,
                bucket: bucket.bucket,
                access_key_id: bucket.access_key_id,
                secret_access_key: bucket.secret_access_key,
            };
            let created = Workspace::create(
                &dir,
                &workspace,
                Profile {
                    name: &name,
                    is_bot: false,
                },
                config,
                Networking::Offline,
            )
            .await?;
            created.sync().await;
            report_sync_error(&created)?;
            println!("Created \"{workspace}\" in {}", dir.display());
            println!("Invite code:\n{}", created.invite_code()?);
        }
        Command::Join { invite, name, bot } => {
            eprintln!("Waiting for the workspace admin's app to accept this member…");
            let joined = Workspace::join(
                &dir,
                &invite,
                Profile {
                    name: &name,
                    is_bot: bot,
                },
                Networking::Offline,
            )
            .await?;
            joined.sync().await;
            report_sync_error(&joined)?;
            println!(
                "Joined \"{}\" as {name}. Data is in {}",
                joined.name(),
                dir.display()
            );
        }
        Command::Invite => {
            let workspace = Workspace::open(&dir, Networking::Offline).await?;
            println!("{}", workspace.invite_code()?);
        }
        Command::Mcp => {
            if !Workspace::exists(&dir) {
                bail!("no workspace in {}; run `shy join` first", dir.display());
            }
            let workspace = Workspace::open(&dir, Networking::Online).await?;
            mcp::serve(workspace).await?;
        }
    }
    Ok(())
}

fn report_sync_error(workspace: &Workspace) -> Result<()> {
    match workspace.status().error {
        Some(error) => bail!("sync failed: {error}"),
        None => Ok(()),
    }
}
