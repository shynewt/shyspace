use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use shy_core::{Networking, views::Change};
use tokio::sync::mpsc;

use crate::{BucketSettings, ChangeListener, DeviceKeyStore, DeviceKeyStoreError, ShyApp, ShyError};

#[derive(Default)]
struct AppKeystore {
    secrets: Mutex<BTreeMap<String, Vec<u8>>>,
    is_locked: bool,
}

impl DeviceKeyStore for AppKeystore {
    fn read(&self, account: String) -> Result<Option<Vec<u8>>, DeviceKeyStoreError> {
        if self.is_locked {
            return Err(DeviceKeyStoreError::Failed {
                reason: "the keystore is locked".into(),
            });
        }
        Ok(self.secrets.lock().unwrap().get(&account).cloned())
    }

    fn create(&self, account: String, secret: Vec<u8>) -> Result<(), DeviceKeyStoreError> {
        let mut secrets = self.secrets.lock().unwrap();
        if secrets.contains_key(&account) {
            return Err(DeviceKeyStoreError::Failed {
                reason: format!("{account} already has a secret"),
            });
        }
        secrets.insert(account, secret);
        Ok(())
    }

    fn delete(&self, account: String) -> Result<(), DeviceKeyStoreError> {
        self.secrets.lock().unwrap().remove(&account);
        Ok(())
    }
}

struct ChangeChannel(mpsc::UnboundedSender<Change>);

impl ChangeListener for ChangeChannel {
    fn on_change(&self, change: Change) {
        let _ = self.0.send(change);
    }
}

fn app_in(dir: &tempfile::TempDir, keystore: &Arc<AppKeystore>) -> Arc<ShyApp> {
    ShyApp::with_memory_bucket(
        dir.path().to_string_lossy().into_owned(),
        keystore.clone(),
        Networking::Offline,
    )
}

fn unused_bucket() -> Arc<BucketSettings> {
    BucketSettings::new(
        "http://unused".into(),
        "us-east-1".into(),
        "unused".into(),
        "unused".into(),
        "unused-secret".into(),
    )
}

async fn create_acme(app: &ShyApp) -> Arc<crate::ShyWorkspace> {
    app.create_workspace("Acme".into(), "Ada".into(), unused_bucket())
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_app_has_no_workspaces() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    assert!(app.open_all().await.unwrap().is_empty());
    assert!(app.workspaces().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_created_workspace_has_a_general_channel_to_write_in() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    let acme = create_acme(&app).await;

    let channels = acme.channels().await.unwrap();
    let general = channels.iter().find(|channel| channel.name == "general").unwrap();
    acme.send_message(general.id.clone(), "hello from a phone".into(), None)
        .await
        .unwrap();

    let messages = acme.messages(general.id.clone(), None, 50).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, "hello from a phone");
    assert_eq!(messages[0].author_name, "Ada");
    let listed = app.workspaces().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Acme");
    assert_eq!(listed[0].workspace_id, acme.workspace_id());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_device_key_lives_in_the_apps_keystore_and_opens_the_workspace_again() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Arc::new(AppKeystore::default());
    let app = app_in(&dir, &keystore);
    let workspace_id = create_acme(&app).await.workspace_id();
    assert_eq!(
        keystore
            .secrets
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        [format!("device-key-{workspace_id}")]
    );
    app.close_all().await.unwrap();
    drop(app);

    let reopened = app_in(&dir, &keystore);
    assert!(reopened.open_all().await.unwrap().is_empty());
    let acme = reopened.workspace(workspace_id.clone()).unwrap();
    assert_eq!(acme.name(), "Acme");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_keystore_is_an_error_the_app_can_show() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Arc::new(AppKeystore {
        is_locked: true,
        ..AppKeystore::default()
    });
    let app = app_in(&dir, &keystore);
    let error = app
        .create_workspace("Acme".into(), "Ada".into(), unused_bucket())
        .await
        .err()
        .unwrap();
    let ShyError::Failed { reason } = error;
    assert!(reason.contains("the keystore is locked"), "{reason}");
}

#[tokio::test(flavor = "multi_thread")]
async fn changes_reach_the_listener_until_the_subscription_ends() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    let acme = create_acme(&app).await;
    let general = acme.channels().await.unwrap()[0].id.clone();
    let (sender, mut changes) = mpsc::unbounded_channel();
    let subscription = acme.subscribe(Arc::new(ChangeChannel(sender)));

    acme.send_message(general.clone(), "hi".into(), None)
        .await
        .unwrap();
    let expected = Change::Messages {
        channel: general.clone(),
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while changes.recv().await.unwrap() != expected {}
    })
    .await
    .expect("the message's change reached the listener");

    subscription.cancel();
    tokio::time::sleep(Duration::from_millis(50)).await;
    while changes.try_recv().is_ok() {}
    acme.send_message(general, "again".into(), None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        changes.try_recv().is_err(),
        "a cancelled subscription stays quiet"
    );
}

async fn general_of(acme: &crate::ShyWorkspace) -> String {
    let channels = acme.channels().await.unwrap();
    channels
        .into_iter()
        .find(|channel| channel.name == "general")
        .unwrap()
        .id
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_can_be_shown_as_plain_text() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    let acme = create_acme(&app).await;

    let plain = acme.plain_texts(vec!["**ship** it `now`".into()]).await.unwrap();

    assert_eq!(plain, ["ship it now"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_can_be_reacted_to_edited_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    let acme = create_acme(&app).await;
    let general = general_of(&acme).await;
    let id = acme
        .send_message(general.clone(), "hello".into(), None)
        .await
        .unwrap();
    let only_message = || async { acme.messages(general.clone(), None, 50).await.unwrap() };

    acme.react(id.clone(), "👍".into(), true).await.unwrap();
    let reactions = &only_message().await[0].reactions;
    assert_eq!(reactions.len(), 1);
    assert_eq!(
        (reactions[0].emoji.as_str(), reactions[0].count, reactions[0].mine),
        ("👍", 1, true)
    );

    acme.react(id.clone(), "👍".into(), false).await.unwrap();
    assert!(only_message().await[0].reactions.is_empty());

    acme.edit_message(id.clone(), "hello, again".into())
        .await
        .unwrap();
    let edited = &only_message().await[0];
    assert_eq!((edited.text.as_str(), edited.is_edited), ("hello, again", true));

    acme.delete_message(id).await.unwrap();
    assert!(only_message().await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn reading_up_to_a_message_moves_the_channels_read_position_to_it() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    let acme = create_acme(&app).await;
    let general = general_of(&acme).await;
    acme.send_message(general.clone(), "first".into(), None)
        .await
        .unwrap();
    let second = acme
        .send_message(general.clone(), "second".into(), None)
        .await
        .unwrap();

    acme.mark_read(general.clone(), second.clone()).await.unwrap();

    let messages = acme.messages(general.clone(), None, 50).await.unwrap();
    let read_up_to = messages.iter().find(|message| message.id == second).unwrap();
    let channel = acme
        .channels()
        .await
        .unwrap()
        .into_iter()
        .find(|channel| channel.id == general)
        .unwrap();
    let last_read = channel.last_read.unwrap();
    assert_eq!(
        (last_read.lamport, last_read.device.as_str()),
        (read_up_to.lamport, read_up_to.device.as_str())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_members_are_listed_with_their_names() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    let acme = create_acme(&app).await;
    let general = general_of(&acme).await;
    acme.send_message(general.clone(), "mine".into(), None)
        .await
        .unwrap();
    let me = acme.messages(general, None, 1).await.unwrap()[0].author.clone();

    let members = acme.members().await.unwrap();

    assert_eq!(members.len(), 1);
    assert_eq!(
        (members[0].id.as_str(), members[0].name.as_str()),
        (me.as_str(), "Ada")
    );
    assert!(members[0].is_admin);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_thread_lists_its_first_message_then_its_replies() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    let acme = create_acme(&app).await;
    let general = general_of(&acme).await;
    let root = acme
        .send_message(general.clone(), "lunch?".into(), None)
        .await
        .unwrap();
    let reply = acme
        .send_message(general.clone(), "yes".into(), Some(root.clone()))
        .await
        .unwrap();

    let thread = acme.thread(root.clone()).await.unwrap();
    assert_eq!(
        thread.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(),
        ["lunch?", "yes"]
    );
    let found = acme.linked_message(reply).await.unwrap().unwrap();
    assert_eq!(found.thread_root, Some(root));
}

#[tokio::test(flavor = "multi_thread")]
async fn marking_a_thread_read_refreshes_its_channel() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    let acme = create_acme(&app).await;
    let general = general_of(&acme).await;
    let root = acme
        .send_message(general.clone(), "lunch?".into(), None)
        .await
        .unwrap();
    let reply = acme
        .send_message(general.clone(), "yes".into(), Some(root.clone()))
        .await
        .unwrap();
    let (sender, mut changes) = mpsc::unbounded_channel();
    let _subscription = acme.subscribe(Arc::new(ChangeChannel(sender)));

    acme.mark_thread_read(root, reply).await.unwrap();

    let expected = Change::Messages { channel: general };
    tokio::time::timeout(Duration::from_secs(5), async {
        while changes.recv().await.unwrap() != expected {}
    })
    .await
    .expect("the thread's channel was told to refresh");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_message_is_no_longer_found() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    let acme = create_acme(&app).await;
    let general = general_of(&acme).await;
    let root = acme.send_message(general, "lunch?".into(), None).await.unwrap();
    assert!(acme.linked_message(root.clone()).await.unwrap().is_some());

    acme.delete_message(root.clone()).await.unwrap();

    assert!(acme.linked_message(root).await.unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_inbox_reaches_the_phones_and_has_nothing_from_yourself() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    let acme = create_acme(&app).await;
    let general = general_of(&acme).await;
    let root = acme
        .send_message(general.clone(), "lunch?".into(), None)
        .await
        .unwrap();
    acme.send_message(general, "yes".into(), Some(root))
        .await
        .unwrap();

    let inbox = acme.inbox().await.unwrap();

    assert!(inbox.attention.is_empty());
    assert!(inbox.channels.is_empty());
    assert!(inbox.page_mentions.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn marking_a_channel_read_refreshes_the_channels() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    let acme = create_acme(&app).await;
    let general = general_of(&acme).await;
    acme.send_message(general.clone(), "hello".into(), None)
        .await
        .unwrap();
    let (sender, mut changes) = mpsc::unbounded_channel();
    let _subscription = acme.subscribe(Arc::new(ChangeChannel(sender)));

    acme.mark_channel_read(general).await.unwrap();

    tokio::time::timeout(Duration::from_secs(5), async {
        while changes.recv().await.unwrap() != Change::Channels {}
    })
    .await
    .expect("the channel list was told to refresh");
}

#[tokio::test(flavor = "multi_thread")]
async fn two_screens_opening_at_once_open_each_workspace_once() {
    let dir = tempfile::tempdir().unwrap();
    let keystore = Arc::new(AppKeystore::default());
    let app = app_in(&dir, &keystore);
    let workspace_id = create_acme(&app).await.workspace_id();
    app.close_all().await.unwrap();
    drop(app);

    for _ in 0..8 {
        let reopened = app_in(&dir, &keystore);
        let (first, second) = tokio::join!(reopened.open_all(), reopened.open_all());
        assert!(first.unwrap().is_empty(), "the first opening failed");
        assert!(second.unwrap().is_empty(), "the second opening failed");
        assert!(reopened.workspace(workspace_id.clone()).is_some());
        reopened.close_all().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pasted_text_that_is_no_link_code_says_so_in_plain_words() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));

    let error = app.read_link_code("hello there".into()).unwrap_err();

    assert_eq!(
        error.to_string(),
        "This isn't a link code. Copy it again from your other device."
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn with_no_link_there_is_nothing_to_resume_or_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    app.open_all().await.unwrap();

    assert!(app.resume_link().await.unwrap().is_none());
    assert!(app.cancel_link().await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_made_here_is_not_reading_only() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(&dir, &Arc::new(AppKeystore::default()));
    let acme = create_acme(&app).await;

    assert!(!acme.is_reading_only());
}
