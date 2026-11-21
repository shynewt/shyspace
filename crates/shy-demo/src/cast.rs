//! The demo's people. The admin creates the workspace; everyone else joins
//! with an invite the admin approves, through the bucket alone (no
//! networking), so building the demo never depends on peers finding each other.

use std::{cell::Cell, path::Path, time::Duration};

use anyhow::{Context, Result, bail};
use shy_core::{BucketConfig, DataDir, KeyStorage, Networking, Profile, Workspace, views::InviteState};

use crate::check_synced;

pub const ADMIN: &str = "Ada";
const WORKSPACE_NAME: &str = "Northwind Studio";
/// Everyone but the admin, in the order they join: (name, is an agent).
const OTHERS: [(&str, bool); 5] = [
    ("Maya", false),
    ("Theo", false),
    ("Priya", false),
    ("Sam", false),
    ("Scout", true),
];
const ASK_POLLS: u32 = 600;
const ASK_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub struct Cast {
    members: Vec<(&'static str, Workspace)>,
    last_to_act: Cell<Option<&'static str>>,
}

impl Cast {
    pub fn get(&self, name: &str) -> &Workspace {
        self.members
            .iter()
            .find(|(member, _)| *member == name)
            .map(|(_, workspace)| workspace)
            .expect("the story only uses people in the cast")
    }

    pub fn id(&self, name: &str) -> String {
        self.get(name).my_id()
    }

    /// `name`, caught up with whoever acted last. Messages are ordered by what
    /// their authors had seen, so without this a reply could come out first.
    pub async fn turn(&self, name: &'static str) -> Result<&Workspace> {
        if let Some(previous) = self.last_to_act.get().filter(|previous| *previous != name) {
            for member in [previous, name] {
                self.get(member).sync().await;
                check_synced(member, self.get(member))?;
            }
        }
        self.last_to_act.set(Some(name));
        Ok(self.get(name))
    }

    /// Two rounds, so what anyone wrote in the first reaches everyone in the second.
    pub async fn sync_everyone(&self) -> Result<()> {
        for _ in 0..2 {
            for (name, member) in &self.members {
                member.sync().await;
                check_synced(name, member)?;
            }
        }
        Ok(())
    }

    /// Uploads what's left and closes every workspace, so the app can open them.
    pub async fn shut_down(self) -> Result<()> {
        for (name, member) in self.members {
            member
                .shutdown()
                .await
                .with_context(|| format!("closing {name}'s workspace"))?;
        }
        Ok(())
    }
}

fn data_dir(root: &Path, name: &str) -> DataDir {
    DataDir::new(root.join(name.to_lowercase()), KeyStorage::Keychain)
}

pub async fn assemble(root: &Path, bucket: BucketConfig) -> Result<Cast> {
    let profile = Profile {
        name: ADMIN,
        is_bot: false,
    };
    let admin = Workspace::create(
        &data_dir(root, ADMIN),
        WORKSPACE_NAME,
        profile,
        bucket,
        Networking::Offline,
    )
    .await
    .context("creating the demo workspace; is the local bucket running? (docs/development.md)")?;
    let mut members = vec![(ADMIN, admin)];
    for (name, is_bot) in OTHERS {
        let member = admit(&members[0].1, root, name, is_bot).await?;
        members.push((name, member));
    }
    let cast = Cast {
        members,
        last_to_act: Cell::new(None),
    };
    cast.sync_everyone().await?;
    Ok(cast)
}

async fn admit(admin: &Workspace, root: &Path, name: &'static str, is_bot: bool) -> Result<Workspace> {
    let code = admin
        .create_invite(name)
        .await
        .with_context(|| format!("inviting {name}"))?;
    let data = data_dir(root, name);
    let joining = tokio::spawn(async move {
        Workspace::join(&data, &code, Profile { name, is_bot }, Networking::Offline).await
    });
    approve_when_asked(admin, name).await?;
    joining
        .await
        .context("the join stopped unexpectedly")?
        .with_context(|| format!("{name} joining"))
}

async fn approve_when_asked(admin: &Workspace, label: &str) -> Result<()> {
    for _ in 0..ASK_POLLS {
        admin.sync().await;
        check_synced(ADMIN, admin)?;
        let invite = admin
            .invites()?
            .into_iter()
            .find(|invite| invite.label == label)
            .with_context(|| format!("the invite for {label} disappeared"))?;
        if invite.state == InviteState::Requested {
            return admin
                .approve_invite(&invite.token)
                .await
                .with_context(|| format!("approving {label}"));
        }
        tokio::time::sleep(ASK_POLL_INTERVAL).await;
    }
    bail!("{label} never asked to join")
}
