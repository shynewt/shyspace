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
