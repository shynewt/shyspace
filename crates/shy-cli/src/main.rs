mod mcp;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use shy_core::{BucketConfig, DataDir, KeyStorage, Networking, Profile, Workspace};

#[derive(Parser)]
#[command(
    name = "shy",
    about = "Headless shyspace member: join workspaces and serve them to AI agents over MCP"
)]
struct Cli {
    /// Where this member's keys and data live.
    #[arg(long, env = "SHY_DIR", global = true)]
    dir: Option<PathBuf>,
    /// Keep the key that encrypts local data in this file instead of the
    /// system keychain (for servers). Keep it outside the data directory.
    #[arg(long, env = "SHY_KEY_FILE", global = true)]
    key_file: Option<PathBuf>,
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
    /// Join a workspace with an invite code. Waits until the admin approves.
    Join {
        invite: String,
        #[arg(long)]
        name: String,
        /// Mark this member as a bot in everyone's member list.
        #[arg(long)]
        bot: bool,
    },
    /// Manage invites (admin only).
    #[command(subcommand)]
    Invite(InviteCommand),
    /// Print the admin's recovery kit. Store it in a password manager.
    RecoveryKit,
    /// Restore the admin onto this device from a recovery kit. Only do this
    /// if the old admin device is gone.
    Restore { kit: String },
    /// Serve the workspace to an AI agent over MCP on stdin/stdout.
    Mcp,
}

#[derive(Subcommand)]
enum InviteCommand {
    /// Create a single-use invite and print its code.
    Create {
        /// A note for yourself, like who it's for.
        #[arg(long, default_value = "")]
        label: String,
    },
    /// List invites and pending join requests.
    List,
    /// Let in the person who requested to join with this invite.
    Approve { token: String },
    /// Decline the request made with this invite.
    Reject { token: String },
    /// Stop this invite from being used.
    Revoke { token: String },
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
    let path = match cli.dir {
        Some(dir) => dir,
        None => default_dir()?,
    };
    let keys = match cli.key_file {
        Some(file) => KeyStorage::File(file),
        None => KeyStorage::Keychain,
    };
    let data = DataDir::new(path, keys);
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
                &data,
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
            println!("Created \"{workspace}\" in {}", data.path.display());
            println!("Invite people with `shy invite create`.");
        }
        Command::Join { invite, name, bot } => {
            eprintln!("Asked to join. Waiting for the workspace admin to approve…");
            let joined = Workspace::join(
                &data,
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
                data.path.display()
            );
        }
        Command::Invite(command) => {
            let workspace = Workspace::open(&data, Networking::Offline).await?;
            run_invite_command(&workspace, command).await?;
        }
        Command::RecoveryKit => {
            let workspace = Workspace::open(&data, Networking::Offline).await?;
            println!("{}", workspace.recovery_kit().await?.as_str());
            eprintln!("Keep this secret: it restores the admin role and holds the bucket credentials.");
        }
        Command::Restore { kit } => {
            let workspace = Workspace::restore(&data, &kit, Networking::Offline).await?;
            println!(
                "Restored the admin of \"{}\" into {}",
                workspace.name(),
                data.path.display()
            );
        }
        Command::Mcp => {
            if !Workspace::exists(&data.path) {
                bail!("no workspace in {}; run `shy join` first", data.path.display());
            }
            let workspace = Workspace::open(&data, Networking::Online).await?;
            mcp::serve(workspace).await?;
        }
    }
    Ok(())
}

async fn run_invite_command(workspace: &Workspace, command: InviteCommand) -> Result<()> {
    // Syncing first picks up requests that arrived since we last looked.
    workspace.sync().await;
    report_sync_error(workspace)?;
    match command {
        InviteCommand::Create { label } => println!("{}", workspace.create_invite(&label).await?),
        InviteCommand::List => print_invites(&workspace.invites()?),
        InviteCommand::Approve { token } => {
            workspace.approve_invite(&token).await?;
            println!("Approved.");
        }
        InviteCommand::Reject { token } => {
            workspace.reject_invite(&token).await?;
            println!("Rejected.");
        }
        InviteCommand::Revoke { token } => {
            workspace.revoke_invite(&token).await?;
            println!("Revoked.");
        }
    }
    Ok(())
}

fn print_invites(invites: &[shy_core::views::Invite]) {
    if invites.is_empty() {
        println!("No invites yet. Create one with `shy invite create`.");
    }
    for invite in invites {
        let label = if invite.label.is_empty() {
            String::new()
        } else {
            format!(" ({})", invite.label)
        };
        let requester = match (&invite.requester_name, &invite.requester_id) {
            (Some(name), Some(id)) => {
                let bot = if invite.requester_is_bot { ", bot" } else { "" };
                format!(" from {name}{bot}, key {}…", &id[..16])
            }
            _ => String::new(),
        };
        println!("{}  {:?}{label}{requester}", invite.token, invite.state);
    }
}

fn report_sync_error(workspace: &Workspace) -> Result<()> {
    match workspace.status().error {
        Some(error) => bail!("sync failed: {error}"),
        None => Ok(()),
    }
}
