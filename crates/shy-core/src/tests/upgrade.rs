//! Opens a workspace the previous release made, and checks every member
//! still sees what they saw there: the stored data is kept, and views
//! rebuilt under new rules come out the same. Ignored by default; run by
//! `scripts/upgrade_check.py`, which makes the workspace with that release
//! (`tools/upgrade-fixture`) and points `SHY_UPGRADE_DIR` at it.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

use crate::{DataDir, KeyStorage, Networking, Workspace};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a workspace made by the previous release: run scripts/upgrade_check.py"]
async fn the_previous_release_s_workspace_opens_unchanged() {
    let root = std::env::var("SHY_UPGRADE_DIR").expect("SHY_UPGRADE_DIR names the fixture's folder");
    let root = Path::new(&root);
    let expected: Value =
        serde_json::from_slice(&std::fs::read(root.join("expected.json")).unwrap()).unwrap();

    // Everyone is compared before anyone writes, so no one sees another's
    // new message.
    let mut members = Vec::new();
    for name in ["ada", "bob"] {
        let member = Workspace::open(&data_dir(root, name), Networking::Offline)
            .await
            .unwrap_or_else(|error| panic!("{name}'s workspace didn't open: {error:#}"));
        assert_same(root, name, &member, &expected[name], "once opened");
        member.sync().await;
        assert_eq!(member.status().error, None, "{name} syncs");
        assert_same(root, name, &member, &expected[name], "after syncing");
        members.push((name, member));
    }
    for (name, member) in members {
        let general = channel_id(&member, "general");
        member
            .send_message(&general, &format!("{name} wrote after updating"), None)
            .unwrap();
        member.sync().await;
        assert_eq!(member.status().error, None, "{name} uploads");
        member.shutdown().await.unwrap();
    }

    let kit = std::fs::read_to_string(root.join("recovery-kit")).unwrap();
    let restored = Workspace::restore(&data_dir(root, "ada-restored"), &kit, Networking::Offline)
        .await
        .expect("the previous release's recovery kit restores");
    let general = channel_id(&restored, "general");
    let texts: Vec<String> = restored
        .messages(&general, None, 500)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect();
    assert!(
        texts.contains(&"Welcome to the upgrade check".to_owned()),
        "{texts:?}"
    );
    assert!(
        texts.contains(&"ada wrote after updating".to_owned()),
        "{texts:?}"
    );
    restored.shutdown().await.unwrap();
}

/// Fails unless `member` sees `expected`, leaving what it saw beside the
/// expected file to compare.
fn assert_same(root: &Path, name: &str, member: &Workspace, expected: &Value, when: &str) {
    let actual = snapshot(member).unwrap();
    if actual != *expected {
        let path = root.join(format!("actual-{name}.json"));
        std::fs::write(&path, serde_json::to_vec_pretty(&actual).unwrap()).unwrap();
        panic!(
            "{name} sees something else {when}; compare {} with expected.json",
            path.display()
        );
    }
}

fn data_dir(root: &Path, name: &str) -> DataDir {
    DataDir::new(
        root.join(name),
        KeyStorage::File(root.join("keys").join(format!("{name}.key"))),
    )
}

fn channel_id(member: &Workspace, name: &str) -> String {
    member
        .channels()
        .unwrap()
        .into_iter()
        .find(|channel| channel.name == name)
        .unwrap_or_else(|| panic!("no #{name}"))
        .id
}

/// What `member` sees of what everyone shares, in a shape both releases
/// make the same way. Kept in step with `snapshot` in
/// `tools/upgrade-fixture`, which the previous release builds.
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
            let fields = ["id", "name", "is_bot", "is_admin", "active", "picture", "status"];
            pick(person, &fields).map(Value::Object)
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
            let fields = ["id", "place", "message", "author", "name", "media_type", "size"];
            pick(file, &fields).map(Value::Object)
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
