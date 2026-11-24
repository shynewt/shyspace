//! Several workspaces on one device (#22): each in its own folder, the
//! single folder of older devices moved once, and leaving.

use std::sync::Arc;

use super::support::*;
use crate::{
    Networking, Profile, Workspace, WorkspaceOptions,
    huddle::media::{DeviceChoice, fake::FakeMedia},
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
async fn a_workspace_with_a_page_open_can_still_be_left() {
    let device = Device::new();
    let workspaces = device.workspaces();
    let acme = workspaces
        .create("Acme", profile(), test_bucket_config(), Networking::Offline)
        .await
        .unwrap();
    let workspace_id = acme.workspace_id().to_owned();
    let page = acme.create_page("Plans", None).unwrap();
    acme.join_page(&page, true).await.unwrap();
    drop(acme);

    workspaces.leave(&workspace_id).await.unwrap();

    assert!(!workspaces.folder(&workspace_id).unwrap().path.exists());
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
            .failed
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

/// Pointed at one workspace folder (as `shy --dir` is, to run several
/// members side by side on one machine), the app holds just that one.
#[tokio::test(flavor = "multi_thread")]
async fn one_workspace_folder_holds_just_that_workspace() {
    let device = Device::new();
    let dir = device.root.path().join("maya");
    let keys = device.root.path().join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    let in_one_folder = || {
        Workspaces::in_one_folder_with_connector(
            &dir,
            DeviceKeys::Files(keys.clone()),
            connector(&device.bucket),
        )
    };
    let workspace_id = in_one_folder()
        .create("Acme", profile(), test_bucket_config(), Networking::Offline)
        .await
        .unwrap()
        .workspace_id()
        .to_owned();
    assert!(dir.join("workspace.json").exists());

    let workspaces = in_one_folder();
    assert!(
        workspaces
            .open_all(|| Networking::Offline.into())
            .await
            .unwrap()
            .failed
            .is_empty()
    );

    assert_eq!(names(&workspaces), ["Acme"]);
    assert_eq!(workspaces.selected().unwrap().workspace_id(), workspace_id);
    let another = workspaces
        .create("Beta", profile(), test_bucket_config(), Networking::Offline)
        .await;
    assert!(another.is_err(), "the folder holds one workspace");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stray_file_beside_the_workspace_folders_doesnt_hide_them() {
    let device = Device::new();
    let workspaces = device.workspaces();
    drop(
        workspaces
            .create("Acme", profile(), test_bucket_config(), Networking::Offline)
            .await
            .unwrap(),
    );
    std::fs::write(device.root.path().join("app/workspaces/.DS_Store"), b"finder").unwrap();

    assert_eq!(device.workspaces().list().map(|list| list.len()).unwrap(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_join_abandoned_in_the_single_folder_doesnt_stop_the_app_starting() {
    let device = Device::new();
    let single = device.single_folder();
    // The join screen shows the safety code first, making an identity.
    single.safety_code().await.unwrap();

    let adopted = device.workspaces().adopt_single_folder(&single).await;

    assert!(adopted.is_ok(), "{:#}", adopted.unwrap_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn leaving_in_one_folder_refuses_an_id_that_isnt_its_workspace() {
    let device = Device::new();
    let dir = device.root.path().join("maya");
    let keys = device.root.path().join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    let in_one_folder = || {
        Workspaces::in_one_folder_with_connector(
            &dir,
            DeviceKeys::Files(keys.clone()),
            connector(&device.bucket),
        )
    };
    drop(
        in_one_folder()
            .create("Acme", profile(), test_bucket_config(), Networking::Offline)
            .await
            .unwrap(),
    );

    let left = in_one_folder().leave("some-other-workspace").await;

    assert!(left.is_err());
    assert!(dir.join("workspace.json").exists(), "Acme's folder was deleted");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_leave_that_cant_finish_keeps_the_workspace_and_it_opens_again() {
    let device = Device::new();
    let workspaces = device.workspaces();
    let acme = workspaces
        .create("Acme", profile(), test_bucket_config(), Networking::Offline)
        .await
        .unwrap();
    let workspace_id = acme.workspace_id().to_owned();

    assert!(
        workspaces.leave(&workspace_id).await.is_err(),
        "still held elsewhere"
    );
    assert!(
        workspaces
            .folder(&workspace_id)
            .unwrap()
            .path
            .join("workspace.json")
            .exists()
    );
    drop(acme);

    workspaces.open_all(|| Networking::Offline.into()).await.unwrap();

    assert!(
        workspaces.find(&workspace_id).is_some(),
        "still on this device, and it opens again"
    );
}

/// A leave cut short while deleting (a file held by another program, or a
/// crash) finishes the next time the workspaces open, key included; until
/// then, the same workspace can't be added again over its old data.
#[tokio::test(flavor = "multi_thread")]
async fn a_leave_cut_short_while_deleting_finishes_later() {
    use crate::workspaces::LeaveStep;
    let device = Device::new();
    let workspaces = device.workspaces();
    let admin = workspaces
        .create("Acme", profile(), test_bucket_config(), Networking::Offline)
        .await
        .unwrap();
    let workspace_id = admin.workspace_id().to_owned();
    let kit = admin.recovery_kit().await.unwrap();
    drop(admin);
    let folder = workspaces.folder(&workspace_id).unwrap();

    workspaces
        .leave_until(&workspace_id, LeaveStep::MarkerDeleted)
        .await
        .unwrap();
    assert!(folder.path.exists() && names(&workspaces).is_empty());
    let Err(again) = workspaces.restore(&kit, Networking::Offline).await else {
        panic!("restored over a folder still being deleted");
    };
    assert!(again.to_string().contains("still being deleted"), "{again:#}");

    let workspaces = device.workspaces();
    workspaces.open_all(|| Networking::Offline.into()).await.unwrap();

    assert!(!folder.path.exists());
    assert!(
        !device
            .root
            .path()
            .join(format!("keys/{workspace_id}.key"))
            .exists()
    );
    workspaces.restore(&kit, Networking::Offline).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn finishing_a_leave_in_one_folder_spares_a_workspace_made_there_since() {
    use crate::workspaces::LeaveStep;
    let device = Device::new();
    let dir = device.root.path().join("maya");
    let keys = device.root.path().join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    let in_one_folder = || {
        Workspaces::in_one_folder_with_connector(
            &dir,
            DeviceKeys::Files(keys.clone()),
            connector(&device.bucket),
        )
    };
    let workspaces = in_one_folder();
    let old_id = workspaces
        .create("Old", profile(), test_bucket_config(), Networking::Offline)
        .await
        .unwrap()
        .workspace_id()
        .to_owned();
    workspaces
        .leave_until(&old_id, LeaveStep::MarkerDeleted)
        .await
        .unwrap();
    // The folder went, but the leave stopped before it was recorded as done.
    std::fs::remove_dir_all(&dir).unwrap();
    drop(
        workspaces
            .create("New", profile(), test_bucket_config(), Networking::Offline)
            .await
            .unwrap(),
    );
    drop(workspaces);

    in_one_folder()
        .open_all(|| Networking::Offline.into())
        .await
        .unwrap();

    assert!(
        dir.join("workspace.json").exists(),
        "the new workspace was deleted"
    );
}

fn with_fake_media(media: &crate::huddle::media::fake::FakeMedia) -> crate::WorkspaceOptions {
    crate::WorkspaceOptions::from(Networking::Offline).with_media(std::sync::Arc::new(media.clone()))
}

/// The one call this device can be in follows its workspace: shown, muted
/// and left from wherever the person is, even with another selected; and
/// joining a call in one workspace leaves the call in the other.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_follows_its_workspace_when_another_is_selected() {
    let device = Device::new();
    let media = crate::huddle::media::fake::FakeMedia::default();
    let workspaces = device.workspaces();
    let acme = workspaces
        .create("Acme", profile(), test_bucket_config(), with_fake_media(&media))
        .await
        .unwrap();
    let beta = workspaces
        .create("Beta", profile(), test_bucket_config(), with_fake_media(&media))
        .await
        .unwrap();
    let acme_general = channel_id(&acme, "general");
    workspaces
        .join_huddle(acme.workspace_id(), &acme_general, false)
        .unwrap();
    workspaces.select(beta.workspace_id()).unwrap();

    let call = workspaces.my_huddle().unwrap().expect("in a call");
    assert_eq!(call.workspace_id, acme.workspace_id());
    assert_eq!(call.channel_name, "#general");
    workspaces.in_huddle().unwrap().set_huddle_muted(true).unwrap();
    assert!(acme.my_huddle().unwrap().muted);

    let beta_general = channel_id(&beta, "general");
    workspaces
        .join_huddle(beta.workspace_id(), &beta_general, false)
        .unwrap();
    assert!(acme.my_huddle().is_none(), "one call at a time");
    assert_eq!(
        workspaces.my_huddle().unwrap().unwrap().workspace_id,
        beta.workspace_id()
    );

    workspaces.in_huddle().unwrap().leave_huddle().unwrap();
    assert!(workspaces.my_huddle().unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn being_away_shows_in_every_workspace() {
    use crate::views::PresenceState;
    let device = Device::new();
    let workspaces = device.workspaces();
    let mut opened = Vec::new();
    for name in ["Acme", "Beta"] {
        let workspace = workspaces
            .create(name, profile(), test_bucket_config(), Networking::Offline)
            .await
            .unwrap();
        opened.push(workspace);
    }

    workspaces.set_away(true);

    for workspace in &opened {
        let now = crate::ops::now_ms().unwrap();
        assert_eq!(workspace.presence_of(&workspace.me(), now), PresenceState::Away);
    }
}

/// Reminders come due in any workspace, each named with its workspace, and
/// the app learns when the next one is due without asking each workspace.
#[tokio::test(flavor = "multi_thread")]
async fn reminders_come_due_from_every_workspace() {
    let device = Device::new();
    let workspaces = device.workspaces();
    let now = crate::ops::now_ms().unwrap();
    let mut ids = Vec::new();
    for (name, remind_ms) in [("Acme", now - 1_000), ("Beta", now - 1_000)] {
        let workspace = workspaces
            .create(name, profile(), test_bucket_config(), Networking::Offline)
            .await
            .unwrap();
        let general = channel_id(&workspace, "general");
        let message = workspace.send_message(&general, "remind me", None).unwrap();
        workspace.save_for_later(&message, Some(remind_ms)).unwrap();
        let later = workspace.send_message(&general, "and later", None).unwrap();
        workspace.save_for_later(&later, Some(now + 60_000)).unwrap();
        ids.push(workspace.workspace_id().to_owned());
    }

    let mut due: Vec<String> = workspaces
        .take_due_reminders()
        .unwrap()
        .into_iter()
        .map(|reminder| reminder.workspace_id)
        .collect();
    due.sort();
    ids.sort();

    assert_eq!(due, ids, "one from each, each with its workspace");
    assert!(
        workspaces.take_due_reminders().unwrap().is_empty(),
        "handed out once"
    );
    assert_eq!(workspaces.next_reminder_ms().unwrap(), Some(now + 60_000));
}

/// The rail's attention count is the window badge's, per workspace:
/// mentions and thread replies outside DMs, unread messages in DMs that
/// aren't muted, and reminders that came due.
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_badge_counts_its_attention_as_the_window_badge_does() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let mention = bob
        .send_message(
            &general,
            &format!("[@Ada](shy://member/{}) look", admin.my_id()),
            None,
        )
        .unwrap();
    let dm = bob.open_dm(&[admin.my_id()]).unwrap();
    bob.send_message(&dm, "one", None).unwrap();
    bob.send_message(&dm, "two", None).unwrap();
    sync_all(&[&bob, &admin]).await;
    // Saving it for later doesn't read it.
    admin
        .save_for_later(&mention, Some(crate::ops::now_ms().unwrap() - 1_000))
        .unwrap();

    assert_eq!(
        admin.badge().unwrap().attention,
        4,
        "1 mention, 2 in the DM, 1 reminder"
    );
}

fn with_audio() -> WorkspaceOptions {
    WorkspaceOptions::from(Networking::Offline).with_media(Arc::new(FakeMedia::default()))
}

fn headset() -> DeviceChoice {
    DeviceChoice {
        input: Some("headset-mic".into()),
        output: Some("headset".into()),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_huddle_devices_are_the_devices_in_every_workspace() {
    let device = Device::new();
    let workspaces = device.workspaces();
    let acme = workspaces
        .create("Acme", profile(), test_bucket_config(), with_audio())
        .await
        .unwrap();
    let beta = workspaces
        .create("Beta", profile(), test_bucket_config(), with_audio())
        .await
        .unwrap();

    acme.set_huddle_devices(headset()).unwrap();

    assert_eq!(beta.huddle_devices().unwrap().chosen, headset());
    drop((acme, beta));
    drop(workspaces);
    let reopened = device.workspaces();
    reopened.open_all(with_audio).await.unwrap();
    for workspace in reopened.open_workspaces() {
        assert_eq!(workspace.huddle_devices().unwrap().chosen, headset());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workspaces_own_device_choice_moves_to_the_device_once() {
    let device = Device::new();
    let workspaces = device.workspaces();
    let acme = workspaces
        .create("Acme", profile(), test_bucket_config(), with_audio())
        .await
        .unwrap();
    let acme_id = acme.workspace_id().to_owned();
    drop(acme);
    drop(workspaces);
    // Saved by the workspace itself, as builds before device-wide choices did.
    let data = device.workspaces().folder(&acme_id).unwrap();
    let alone = Workspace::open_with(&data, connector(&device.bucket), with_audio())
        .await
        .unwrap();
    alone.set_huddle_devices(headset()).unwrap();
    drop(alone);

    let workspaces = device.workspaces();
    workspaces.open_all(with_audio).await.unwrap();
    let beta = workspaces
        .create("Beta", profile(), test_bucket_config(), with_audio())
        .await
        .unwrap();
    let acme = workspaces
        .open_workspaces()
        .into_iter()
        .find(|workspace| workspace.workspace_id() == acme_id)
        .unwrap();
    assert_eq!(acme.huddle_devices().unwrap().chosen, headset());
    assert_eq!(beta.huddle_devices().unwrap().chosen, headset());
}

#[tokio::test(flavor = "multi_thread")]
async fn huddle_sounds_are_the_devices_in_every_workspace() {
    let device = Device::new();
    let workspaces = device.workspaces();
    let acme = workspaces
        .create("Acme", profile(), test_bucket_config(), with_audio())
        .await
        .unwrap();
    let beta = workspaces
        .create("Beta", profile(), test_bucket_config(), with_audio())
        .await
        .unwrap();
    assert!(beta.huddle_sounds().unwrap(), "on until turned off");

    acme.set_huddle_sounds(false).unwrap();

    assert!(!beta.huddle_sounds().unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workspaces_own_sounds_setting_moves_to_the_device_once() {
    let device = Device::new();
    let workspaces = device.workspaces();
    let acme = workspaces
        .create("Acme", profile(), test_bucket_config(), with_audio())
        .await
        .unwrap();
    let acme_id = acme.workspace_id().to_owned();
    drop(acme);
    drop(workspaces);
    let data = device.workspaces().folder(&acme_id).unwrap();
    let alone = Workspace::open_with(&data, connector(&device.bucket), with_audio())
        .await
        .unwrap();
    alone.set_huddle_sounds(false).unwrap();
    drop(alone);

    let workspaces = device.workspaces();
    workspaces.open_all(with_audio).await.unwrap();
    let acme = workspaces.open_workspaces().into_iter().next().unwrap();
    assert!(!acme.huddle_sounds().unwrap());
    let beta = workspaces
        .create("Beta", profile(), test_bucket_config(), with_audio())
        .await
        .unwrap();
    assert!(!beta.huddle_sounds().unwrap());
}
