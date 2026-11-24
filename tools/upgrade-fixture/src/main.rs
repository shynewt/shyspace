//! Makes a workspace with the previous release, so the current one can be
//! checked against it (`scripts/upgrade_check.py`). Built inside a checkout
//! of that release, against its shy-core, so it uses only what that
//! release offers.
//!
//! Ada (the admin), Bob and Carol exchange messages, a reply, an edit, a
//! reaction and a delete; Ada makes a private channel, a DM and a page with
//! an image; Bob shares a file and sets a picture and status; Carol writes,
//! then is removed. It then writes what Ada and Bob see to `expected.json`
//! (the shape `tests/upgrade.rs` in shy-core reads) and Ada's recovery kit
//! to `recovery-kit`. Keys are kept in files under `keys/`, never in the
//! system keychain.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Value, json};
use shy_core::{
    BucketConfig, DataDir, Edit, KeyStorage, Networking, Profile, Workspace,
    views::{InviteState, NotifyLevel, StatusUpdate},
    workspace::{FileSource, NewFile},
};

const ASK_POLLS: u32 = 600;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nupgrade-check";

#[tokio::main]
async fn main() -> Result<()> {
    let root: PathBuf = std::env::args_os()
        .nth(1)
        .context("usage: upgrade-fixture <empty directory>")?
        .into();
    ensure!(
        !root.exists() || std::fs::read_dir(&root)?.next().is_none(),
        "{} isn't empty",
        root.display()
    );
    std::fs::create_dir_all(root.join("keys"))?;

    let ada = Workspace::create(
        &data_dir(&root, "ada"),
        "Upgrade check",
        Profile {
            name: "Ada",
            is_bot: false,
        },
        bucket_from_env(),
        Networking::Offline,
    )
    .await
    .context("creating the workspace; is the local bucket running? (docs/development.md)")?;
    let bob = admit(&ada, &root, "Bob").await?;
    let carol = admit(&ada, &root, "Carol").await?;
    let everyone = [&ada, &bob, &carol];
    sync(&everyone).await?;

    let general = ada
        .channels()?
        .into_iter()
        .find(|channel| channel.name == "general")
        .context("no #general")?
        .id;
    let welcome = ada.send_message(&general, "Welcome to the upgrade check", None)?;
    let gone = ada.send_message(&general, "This one is deleted", None)?;
    ada.delete_message(&gone)?;
    sync(&everyone).await?;
    let reply = bob.send_message(&general, "A reply", Some(welcome.clone()))?;
    bob.edit_message(&reply, "An edited reply")?;
    carol.react(&welcome, "👍", true)?;
    sync(&everyone).await?;

    let plans = ada.create_private_channel("plans", &[bob.my_id()])?;
    ada.send_message(&plans, "A private plan", None)?;
    let dm = ada.open_dm(&[bob.my_id()])?;
    ada.send_message(&dm, "Just for you", None)?;
    let page = ada.create_page("Handbook", None)?;
    ada.edit_page(&page, Edit::Append("# Handbook\n\nHow we work.\n"))?;
    let image = ada
        .share_page_file(&page, new_file("diagram.png", "image/png", PNG))
        .await?;
    ada.edit_page(&page, Edit::Append(&format!("\n![](shy://file/{image})\n")))?;
    sync(&everyone).await?;
    bob.send_message(&dm, "Got it", None)?;
    bob.send_files(
        &general,
        "Notes attached",
        None,
        vec![new_file("notes.txt", "text/plain", b"upgrade notes")],
    )
    .await?;
    bob.set_profile_picture(Some(PNG.to_vec())).await?;
    bob.set_status(StatusUpdate {
        emoji: "🌴",
        text: "Away",
        until_ms: None,
        alerts: NotifyLevel::Mentions,
    })?;
    carol.send_message(&general, "Carol's last words", None)?;
    sync(&everyone).await?;
    ada.remove_member(&carol.my_id()).await?;
    sync(&[&ada, &bob]).await?;
    sync(&[&ada, &bob]).await?;

    let kit = ada.recovery_kit().await?;
    std::fs::write(root.join("recovery-kit"), kit.as_bytes())?;
    let expected = json!({ "ada": snapshot(&ada)?, "bob": snapshot(&bob)? });
    std::fs::write(root.join("expected.json"), serde_json::to_vec_pretty(&expected)?)?;
    for (name, member) in [("Ada", ada), ("Bob", bob), ("Carol", carol)] {
        member
            .shutdown()
            .await
            .with_context(|| format!("closing {name}'s workspace"))?;
    }
    println!("made the workspace in {}", root.display());
    Ok(())
}

fn data_dir(root: &Path, name: &str) -> DataDir {
    DataDir::new(
        root.join(name),
        KeyStorage::File(root.join("keys").join(format!("{name}.key"))),
    )
}

/// The bucket from the `SHY_S3_*` variables the CLI reads, else the local one.
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

fn new_file(name: &str, media_type: &str, bytes: &[u8]) -> NewFile {
    NewFile {
        name: name.to_owned(),
        media_type: media_type.to_owned(),
        source: FileSource::Bytes(bytes.to_vec()),
    }
}

/// Two rounds, so what anyone wrote in the first reaches everyone in the second.
async fn sync(members: &[&Workspace]) -> Result<()> {
    for _ in 0..2 {
        for member in members {
            member.sync().await;
            if let Some(error) = member.status().error {
                bail!("a member couldn't sync: {error}");
            }
        }
    }
    Ok(())
}

async fn admit(admin: &Workspace, root: &Path, name: &'static str) -> Result<Workspace> {
    let code = admin.create_invite(name).await?;
    let data = data_dir(root, &name.to_lowercase());
    let joining = tokio::spawn(async move {
        Workspace::join(&data, &code, Profile { name, is_bot: false }, Networking::Offline).await
    });
    for _ in 0..ASK_POLLS {
        admin.sync().await;
        let invite = admin
            .invites()?
            .into_iter()
            .find(|invite| invite.label == name)
            .with_context(|| format!("the invite for {name} disappeared"))?;
        if invite.state == InviteState::Requested {
            admin.approve_invite(&invite.token).await?;
            return joining.await.context("the join stopped")?;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    bail!("{name} never asked to join")
}

/// What `member` sees of what everyone shares, in a shape both releases
/// make the same way. Kept in step with `snapshot` in shy-core's
/// `tests/upgrade.rs`.
fn snapshot(member: &Workspace) -> Result<Value> {
    let mut channels = Vec::new();
    for channel in member.channels()? {
        let mut picked = pick(&channel, &["id", "kind", "name", "dm_members", "is_member"])?;
        if channel.is_member && !channel.is_missing_key {
            let mut messages = Vec::new();
            for message in member.messages(&channel.id, None, 500)? {
                if message.reply_count > 0 {
                    for reply in member.thread(&message.id)? {
                        messages.push(pick_message(&reply)?);
                    }
                }
                messages.push(pick_message(&message)?);
            }
            sort_by_id(&mut messages);
            messages.dedup_by(|left, right| left["id"] == right["id"]);
            picked.insert("messages".into(), Value::Array(messages));
        }
        channels.push(Value::Object(picked));
    }
    sort_by_id(&mut channels);
    let mut members = member
        .members()?
        .iter()
        .map(|person| {
            pick(
                person,
                &["id", "name", "is_bot", "is_admin", "active", "picture", "status"],
            )
            .map(Value::Object)
        })
        .collect::<Result<Vec<_>>>()?;
    sort_by_id(&mut members);
    let mut pages = Vec::new();
    for page in member.pages()? {
        let mut picked = pick(&page, &["id", "parent", "title"])?;
        picked.insert("text".into(), Value::String(member.page_text(&page.id)?));
        pages.push(Value::Object(picked));
    }
    sort_by_id(&mut pages);
    let mut files = member
        .files()?
        .iter()
        .map(|file| {
            pick(
                file,
                &["id", "place", "message", "author", "name", "media_type", "size"],
            )
            .map(Value::Object)
        })
        .collect::<Result<Vec<_>>>()?;
    sort_by_id(&mut files);
    Ok(json!({ "channels": channels, "members": members, "pages": pages, "files": files }))
}

fn pick_message(message: &impl serde::Serialize) -> Result<Value> {
    let fields = [
        "id",
        "author",
        "text",
        "reply_to",
        "is_edited",
        "reactions",
        "files",
    ];
    pick(message, &fields).map(Value::Object)
}

fn pick(view: &impl serde::Serialize, fields: &[&str]) -> Result<Map<String, Value>> {
    let Value::Object(all) = serde_json::to_value(view)? else {
        bail!("a view isn't an object");
    };
    fields
        .iter()
        .map(|field| {
            let value = all.get(*field).with_context(|| format!("no field {field}"))?;
            Ok(((*field).to_owned(), value.clone()))
        })
        .collect()
}

fn sort_by_id(values: &mut [Value]) {
    values.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
}
