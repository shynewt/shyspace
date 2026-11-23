//! Several workspaces on one device (#22): each in its own folder, the
//! single folder of older devices moved once, and leaving.

use super::support::*;
use crate::{
    Networking, Profile, Workspace,
    workspaces::{DeviceKeys, MoveStep, Workspaces},
};

fn profile() -> Profile<'static> {
    Profile {
        name: "Ada",
        is_bot: false,
    }
}

struct Device {
    root: tempfile::TempDir,
    bucket: crate::transport::bucket::Bucket,
}

impl Device {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            bucket: memory_bucket(),
        }
    }

    fn workspaces(&self) -> Workspaces {
        let keys = self.root.path().join("keys");
        std::fs::create_dir_all(&keys).unwrap();
        Workspaces::with_connector(
            self.root.path().join("app"),
            DeviceKeys::Files(keys),
            connector(&self.bucket),
        )
    }

    /// The one folder a device had before there could be several.
    fn single_folder(&self) -> crate::DataDir {
        std::fs::create_dir_all(self.root.path().join("app")).unwrap();
        crate::DataDir::new(
            self.root.path().join("app/workspace"),
            crate::keys::KeyStorage::File(self.root.path().join("single.key")),
        )
    }

    async fn open(&self, workspace_id: &str) -> Workspace {
        let data = self.workspaces().folder(workspace_id).unwrap();
        Workspace::open_with(&data, connector(&self.bucket), Networking::Offline)
            .await
            .unwrap()
    }
}

fn names(workspaces: &Workspaces) -> Vec<String> {
    workspaces
        .list()
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn each_workspace_lives_in_its_own_folder_in_the_order_arranged() {
    let device = Device::new();
    let workspaces = device.workspaces();
    let acme = workspaces
        .create("Acme", profile(), test_bucket_config(), Networking::Offline)
        .await
        .unwrap();
    let beta = workspaces
        .create("Beta", profile(), test_bucket_config(), Networking::Offline)
        .await
        .unwrap();
    assert_ne!(acme.me(), beta.me(), "each workspace has its own identity");
    assert_eq!(names(&workspaces), ["Acme", "Beta"]);

    workspaces.move_to(beta.workspace_id(), 0).unwrap();

    assert_eq!(names(&workspaces), ["Beta", "Acme"]);
    assert!(
        workspaces.folder("../elsewhere").is_err(),
        "an id is never a path"
    );
    for workspace in [&acme, &beta] {
        let folder = workspaces.folder(workspace.workspace_id()).unwrap();
        assert!(folder.path.join("workspace.json").exists());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_single_folder_of_an_older_device_moves_once_and_keeps_everything() {
    let device = Device::new();
    let single = device.single_folder();
    let workspace_id = {
        let admin = Workspace::create_with(
            &single,
            "moved-workspace".into(),
            "Acme",
            profile(),
            test_bucket_config(),
            connector(&device.bucket),
            Networking::Offline,
        )
        .await
        .unwrap();
        let general = channel_id(&admin, "general");
        admin
            .send_message(&general, "from before the move", None)
            .unwrap();
        admin.workspace_id().to_owned()
    };
    let workspaces = device.workspaces();

    assert_eq!(
        workspaces.adopt_single_folder(&single).await.unwrap(),
        Some(workspace_id.clone())
    );
    assert_eq!(
        workspaces.adopt_single_folder(&single).await.unwrap(),
        None,
        "only once"
    );

    assert!(!single.path.exists());
    assert!(
        !device.root.path().join("single.key").exists(),
        "the old key is gone"
    );
    assert_eq!(names(&workspaces), ["Acme"]);
    let moved = device.open(&workspace_id).await;
    let general = channel_id(&moved, "general");
    assert_eq!(texts_in(&moved, &general), ["from before the move"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_move_cut_short_at_any_step_finishes_the_next_time() {
    for step in [MoveStep::KeyCopied, MoveStep::FolderMoved] {
        let device = Device::new();
        let single = device.single_folder();
        let workspace_id = Workspace::create_with(
            &single,
            "moved-workspace".into(),
            "Acme",
            profile(),
            test_bucket_config(),
            connector(&device.bucket),
            Networking::Offline,
        )
        .await
        .unwrap()
        .workspace_id()
        .to_owned();
        let workspaces = device.workspaces();
        workspaces.move_single_folder(&single, step).await.unwrap();

        workspaces.adopt_single_folder(&single).await.unwrap();

        assert!(!single.path.exists());
        assert!(
            !device.root.path().join("single.key").exists(),
            "the old key is gone"
        );
        assert_eq!(names(&workspaces), ["Acme"]);
        device.open(&workspace_id).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn leaving_closes_the_workspace_and_deletes_its_folder_and_key() {
    let device = Device::new();
    let workspaces = device.workspaces();
    let acme = workspaces
        .create("Acme", profile(), test_bucket_config(), Networking::Offline)
        .await
        .unwrap();
    let workspace_id = acme.workspace_id().to_owned();
    let folder = workspaces.folder(&workspace_id).unwrap();

    assert!(
        workspaces.leave(&workspace_id).await.is_err(),
        "still open elsewhere (here, the handle the test holds)"
    );
    drop(acme);
    let acme = workspaces
        .create("Acme 2", profile(), test_bucket_config(), Networking::Offline)
        .await
        .unwrap();
    let second_id = acme.workspace_id().to_owned();
    drop(acme);
    workspaces.leave(&second_id).await.unwrap();

    assert!(!workspaces.folder(&second_id).unwrap().path.exists());
    assert!(!device.root.path().join(format!("keys/{second_id}.key")).exists());
    assert!(folder.path.exists(), "the first is untouched");
    assert_eq!(names(&workspaces), ["Acme"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn leaving_the_selected_workspace_selects_the_one_used_before() {
    let device = Device::new();
    let workspaces = device.workspaces();
    let acme = workspaces
        .create("Acme", profile(), test_bucket_config(), Networking::Offline)
        .await
        .unwrap()
        .workspace_id()
        .to_owned();
    let beta = workspaces
        .create("Beta", profile(), test_bucket_config(), Networking::Offline)
        .await
        .unwrap()
        .workspace_id()
        .to_owned();
    assert_eq!(workspaces.selected().unwrap().workspace_id(), beta);

    workspaces.leave(&beta).await.unwrap();

    assert_eq!(workspaces.selected().unwrap().workspace_id(), acme);
}

/// Every workspace stays open, gossip delivering live; only the most
/// recently used check the bucket at full pace, also after a restart.
#[tokio::test(flavor = "multi_thread")]
async fn the_most_recently_used_workspaces_run_live() {
    use crate::{sync::Cadence, workspaces::LIVE_WORKSPACES};
    let device = Device::new();
    let mut ids = Vec::new();
    {
        let workspaces = device.workspaces();
        for number in 0..LIVE_WORKSPACES + 2 {
            let workspace = workspaces
                .create(
                    &format!("W{number}"),
                    profile(),
                    test_bucket_config(),
                    Networking::Offline,
                )
                .await
                .unwrap();
            ids.push(workspace.workspace_id().to_owned());
        }
        workspaces.select(&ids[0]).unwrap();
        let live: Vec<bool> = workspaces
            .rail()
            .unwrap()
            .iter()
            .map(|item| item.is_live)
            .collect();
        // W0 was just used, then W6 back to W3; W1 and W2 least recently.
        assert_eq!(live, [true, false, false, true, true, true, true]);
    }

    let workspaces = device.workspaces();
    assert!(
        workspaces
            .open_all(|| Networking::Offline.into())
            .await
            .unwrap()
            .is_empty()
    );

    assert_eq!(workspaces.selected().unwrap().workspace_id(), ids[0]);
    let cadence_of = |id: &str| workspaces.find(id).unwrap().cadence();
    assert_eq!(cadence_of(&ids[0]), Cadence::Live);
    assert_eq!(cadence_of(&ids[1]), Cadence::Background);
}

#[tokio::test(flavor = "multi_thread")]
async fn muting_a_workspace_or_pausing_all_holds_its_notifications() {
    let device = Device::new();
    let workspaces = device.workspaces();
    let acme = workspaces
        .create("Acme", profile(), test_bucket_config(), Networking::Offline)
        .await
        .unwrap()
        .workspace_id()
        .to_owned();
    assert!(workspaces.may_notify(&acme).unwrap());

    workspaces.mute(&acme).unwrap();
    assert!(!workspaces.may_notify(&acme).unwrap());
    assert!(workspaces.rail().unwrap()[0].is_muted);
    workspaces.unmute(&acme).unwrap();

    let now = crate::ops::now_ms().unwrap();
    workspaces.pause_notifications(now + 60_000).unwrap();
    assert!(!workspaces.may_notify(&acme).unwrap());
    workspaces.pause_notifications(now - 1).unwrap();
    assert!(workspaces.may_notify(&acme).unwrap(), "a pause that ran out");
    assert_eq!(workspaces.notifications_paused().unwrap(), None);
}

/// The rail's badge counts what the workspace's sidebar does: mentions,
/// and every unread message in a DM that isn't muted.
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_badge_counts_what_its_sidebar_does() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "no mention here", None).unwrap();
    bob.send_message(
        &general,
        &format!("[@Ada](shy://member/{}) look", admin.my_id()),
        None,
    )
    .unwrap();
    let dm = bob.open_dm(&[admin.my_id()]).unwrap();
    bob.send_message(&dm, "just for you", None).unwrap();
    sync_all(&[&bob, &admin]).await;

    let badge = admin.badge().unwrap();
    assert!(badge.unread);
    assert_eq!(badge.mentions, 2, "one mention and one DM message");

    admin
        .set_notify_level(&dm, crate::views::NotifyLevel::Nothing)
        .unwrap();
    assert_eq!(admin.badge().unwrap().mentions, 1, "a muted DM counts nothing");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_already_on_this_device_isnt_added_again() {
    let device = Device::new();
    let workspaces = device.workspaces();
    let admin = workspaces
        .create("Acme", profile(), test_bucket_config(), Networking::Offline)
        .await
        .unwrap();
    let kit = admin.recovery_kit().await.unwrap();

    let Err(again) = workspaces.restore(&kit, Networking::Offline).await else {
        panic!("restored a workspace already here");
    };

    assert!(again.to_string().contains("already on this device"), "{again:#}");
    assert_eq!(names(&workspaces), ["Acme"]);
}
