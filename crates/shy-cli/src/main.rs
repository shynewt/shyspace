mod agents;
mod mcp;
mod skill;

use std::{io::IsTerminal, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use shy_core::{BucketConfig, DataDir, KeyStorage, Networking, Profile, Workspace, workspace::StorageUpdate};

const RECOVERY_KIT_VARIABLE: &str = "SHY_RECOVERY_KIT";
const INVITE_VARIABLE: &str = "SHY_INVITE";

#[derive(Parser)]
#[command(
    name = "shy",
    about = "Headless shyspace member: join workspaces and serve them to AI agents over MCP"
)]
struct Cli {
    /// Where this member's keys and data live.
    #[arg(long, env = "SHY_DIR", global = true, conflicts_with = "agent")]
    dir: Option<PathBuf>,
    /// Act as the named agent: its own member, with its own identity and
    /// data folder. Use one name per agent (and per concurrent session).
    #[arg(long, env = "SHY_AGENT", global = true)]
    agent: Option<String>,
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
    /// Leave the code off to keep it out of shell history: it is then read
    /// from SHY_INVITE or standard input.
    Join {
        invite: Option<String>,
        #[arg(long)]
        name: String,
        /// Mark this member as a bot in everyone's member list.
        #[arg(long)]
        bot: bool,
    },
    /// Manage invites (admin only).
    #[command(subcommand)]
    Invite(InviteCommand),
    /// Show or change the bucket settings (changing is admin only).
    #[command(subcommand)]
    Storage(StorageCommand),
    /// Print the admin's recovery kit. Store it in a password manager.
    RecoveryKit,
    /// Restore the admin onto this device from a recovery kit. Only do this
    /// if the old admin device is gone. The kit is read from standard input
    /// (or SHY_RECOVERY_KIT), never the command line, where shell history
    /// and other users could see it.
    Restore,
    /// Serve the workspace to an AI agent over MCP on stdin/stdout.
    Mcp,
    /// List the named agents on this machine.
    Agents,
    /// Print the agent skill (how to be a good member). For Claude Code:
    /// `mkdir -p ~/.claude/skills/shyspace && shy skill > ~/.claude/skills/shyspace/SKILL.md`
    Skill,
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

#[derive(Subcommand)]
enum StorageCommand {
    /// Show the bucket settings (never the secret key).
    Show,
    /// Change the region and keys, for example after rotating them. Keep the
    /// old key active until every member has synced.
    Update {
        #[arg(long, env = "SHY_S3_REGION")]
        region: String,
        #[arg(long, env = "SHY_S3_ACCESS_KEY_ID")]
        access_key_id: String,
        /// Prefer the environment variable, to keep it out of shell history.
        #[arg(long, env = "SHY_S3_SECRET_ACCESS_KEY", hide_env_values = true)]
        secret_access_key: String,
    },
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
    /// Prefer the environment variable, to keep it out of shell history.
    #[arg(long, env = "SHY_S3_SECRET_ACCESS_KEY", hide_env_values = true)]
    secret_access_key: String,
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
    let path = match (cli.dir, &cli.agent) {
        (Some(dir), _) => dir,
        (None, Some(agent)) => agents::agent_dir(agent)?,
        (None, None) => agents::default_member_dir()?,
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
            let invite = match invite {
                Some(invite) => invite,
                None => read_secret(INVITE_VARIABLE, "invite code")?,
            };
            let code = data.safety_code().await?;
            eprintln!("Asking to join. Your safety code is {code}");
            eprintln!("The admin should see the same code before approving. Waiting…");
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
        Command::Storage(command) => {
            let workspace = Workspace::open(&data, Networking::Offline).await?;
            run_storage_command(&workspace, command).await?;
        }
        Command::RecoveryKit => {
            let workspace = Workspace::open(&data, Networking::Offline).await?;
            println!("{}", workspace.recovery_kit().await?.as_str());
            eprintln!("Keep this secret: it restores the admin role and holds the bucket credentials.");
        }
        Command::Restore => {
            let kit = read_secret(RECOVERY_KIT_VARIABLE, "recovery kit")?;
            let workspace = Workspace::restore(&data, &kit, Networking::Offline).await?;
            println!(
                "Restored the admin of \"{}\" into {}",
                workspace.name(),
                data.path.display()
            );
        }
        Command::Agents => {
            let agents = agents::list()?;
            if agents.is_empty() {
                println!(
                    "No named agents yet. Create one with `shy --agent <name> join --bot --name <Name>` and paste the invite."
                );
            }
            for (name, workspace) in agents {
                println!(
                    "{name}  {}",
                    workspace.map_or("(not in a workspace yet)".into(), |name| format!("in \"{name}\""))
                );
            }
        }
        Command::Skill => print!("{}", skill::SKILL),
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

async fn run_storage_command(workspace: &Workspace, command: StorageCommand) -> Result<()> {
    match command {
        StorageCommand::Show => {
            let settings = workspace.storage_settings()?;
            println!("Endpoint:   {}", settings.endpoint);
            println!("Bucket:     {}", settings.bucket);
            println!("Region:     {}", settings.region);
            println!("Access key: {}", settings.access_key_id);
        }
        StorageCommand::Update {
            region,
            access_key_id,
            secret_access_key,
        } => {
            let update = StorageUpdate {
                region: &region,
                access_key_id: &access_key_id,
                secret_access_key: &secret_access_key,
            };
            workspace.update_storage(update).await?;
            workspace.sync().await;
            report_sync_error(workspace)?;
            println!("Updated. Keep the old key active until every member has synced.");
            if workspace.has_recovery_kit()? {
                println!(
                    "Your recovery kit changed; save the new one:\n{}",
                    workspace.recovery_kit().await?.as_str()
                );
            }
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
                let code = invite.requester_safety_code.as_deref().unwrap_or("?");
                format!(" from {name}{bot} (key {}…, safety code {code})", &id[..16])
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

/// The recovery kit from `SHY_RECOVERY_KIT`, or else the first line of
/// standard input.
/// Reads a secret from `variable` or else one line of standard input, so
/// it never sits in shell history or other users' process lists.
fn read_secret(variable: &str, what: &str) -> Result<String> {
    let from_variable = std::env::var(variable).ok();
    let is_variable_blank = from_variable
        .as_deref()
        .is_none_or(|value| value.trim().is_empty());
    if is_variable_blank && std::io::stdin().is_terminal() {
        eprintln!("Paste the {what}, then press Enter:");
    }
    secret_from(from_variable, std::io::stdin().lock(), variable, what)
}

fn secret_from(
    from_variable: Option<String>,
    mut input: impl std::io::BufRead,
    variable: &str,
    what: &str,
) -> Result<String> {
    if let Some(value) = from_variable.filter(|value| !value.trim().is_empty()) {
        return Ok(value.trim().to_owned());
    }
    let mut line = String::new();
    input
        .read_line(&mut line)
        .with_context(|| format!("reading the {what} from standard input"))?;
    let line = line.trim();
    if line.is_empty() {
        bail!("no {what}: pipe it to standard input or set {variable}");
    }
    Ok(line.to_owned())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    /// `--help` never prints a secret taken from the environment, and the
    /// recovery kit is never an argument (shell history, other users'
    /// process lists).
    #[test]
    fn help_hides_secrets_and_the_kit_is_never_an_argument() {
        let cli = Cli::command();
        let mut secrets = 0;
        let mut commands = vec![&cli];
        while let Some(command) = commands.pop() {
            commands.extend(command.get_subcommands());
            for argument in command.get_arguments() {
                if argument.get_id().as_str().contains("secret") {
                    secrets += 1;
                    assert!(
                        argument.is_hide_env_values_set(),
                        "{} shows its value",
                        argument.get_id()
                    );
                }
            }
        }
        assert!(secrets >= 2, "found {secrets} secret arguments");
        let restore = cli.find_subcommand("restore").unwrap();
        assert_eq!(restore.get_arguments().count(), 0, "the kit isn't an argument");
    }

    /// The invite can stay out of shell history: left off the command line,
    /// it is read from SHY_INVITE or standard input.
    #[test]
    fn join_works_without_the_invite_on_the_command_line() {
        let cli = Cli::try_parse_from(["shy", "join", "--name", "Claude"]).unwrap();
        assert!(matches!(cli.command, Command::Join { invite: None, .. }));
    }

    #[test]
    fn a_secret_comes_from_the_variable_first_then_one_line_of_input() {
        let from_variable = secret_from(
            Some(" shy1abc \n".into()),
            &b"ignored\n"[..],
            INVITE_VARIABLE,
            "invite",
        );
        assert_eq!(from_variable.unwrap(), "shy1abc");

        let from_input = secret_from(None, &b"  shy1def\nsecond line\n"[..], INVITE_VARIABLE, "invite");
        assert_eq!(from_input.unwrap(), "shy1def");

        let blank_variable = secret_from(Some("  ".into()), &b"shy1ghi\n"[..], INVITE_VARIABLE, "invite");
        assert_eq!(blank_variable.unwrap(), "shy1ghi");

        let error = secret_from(None, &b"\n"[..], INVITE_VARIABLE, "invite").unwrap_err();
        assert!(error.to_string().contains(INVITE_VARIABLE), "{error}");
    }
}
