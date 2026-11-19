//! The admin changing the bucket's region or keys.

use std::sync::Arc;

use super::support::*;
use crate::{
    crypto,
    ops::{Op, OpBody},
    transport::bucket::{BucketConfig, Connector},
    workspace::StorageUpdate,
};

fn rotated() -> StorageUpdate<'static> {
    StorageUpdate {
        region: "us-west-000",
        access_key_id: "NEWKEY",
        secret_access_key: "new-secret",
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn new_storage_settings_reach_every_member() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    admin.update_storage(rotated()).await.unwrap();
    sync_all(&[&admin, &bob]).await;
    for member in [&admin, &bob] {
        let settings = member.storage_settings().unwrap();
        assert_eq!(settings.access_key_id, "NEWKEY");
        assert_eq!(settings.region, "us-west-000");
        assert_eq!(
            settings.endpoint,
            test_bucket_config().endpoint,
            "the endpoint can't change"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_admin_changes_storage_settings() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    assert!(bob.update_storage(rotated()).await.is_err());

    // A forged settings op from Bob is skipped, and doesn't block his later messages.
    let (epoch, key) = current_key(&bob);
    let seq = bob.state().store.next_seq(&bob.me()).unwrap();
    let forged = OpBody {
        lamport: 50_000,
        timestamp_ms: 0,
        op: Op::StorageSettings {
            bucket: BucketConfig {
                access_key_id: "BOBS".into(),
                ..test_bucket_config()
            },
        },
    };
    admin
        .receive(crypto::seal(&bob.inner.identity, &key, epoch, seq, &forged).unwrap())
        .unwrap();
    assert_eq!(
        admin.storage_settings().unwrap().access_key_id,
        test_bucket_config().access_key_id
    );

    let general = channel_id(&admin, "general");
    let next = OpBody {
        lamport: 50_001,
        timestamp_ms: 0,
        op: Op::Message {
            id: crate::ops::new_owned_id(&bob.me()),
            channel: general.clone(),
            text: "still heard".into(),
            reply_to: None,
        },
    };
    admin
        .receive(crypto::seal(&bob.inner.identity, &key, epoch, seq + 1, &next).unwrap())
        .unwrap();
    assert!(
        admin
            .messages(&general, None, 50)
            .unwrap()
            .iter()
            .any(|m| m.text == "still heard")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn removed_members_never_get_new_keys() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    admin.remove_member(&bob.my_id()).await.unwrap();
    admin.update_storage(rotated()).await.unwrap();
    sync_all(&[&admin]).await;
    bob.sync().await;
    assert_eq!(
        bob.storage_settings().unwrap().access_key_id,
        test_bucket_config().access_key_id
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_that_fail_the_check_change_nothing() {
    let root = tempfile::tempdir().unwrap();
    let bucket = memory_bucket();
    let working = bucket.clone();
    let rejects_bad_keys: Connector = Arc::new(move |_: &str, config: &BucketConfig| {
        anyhow::ensure!(config.access_key_id != "BADKEY", "403 Forbidden: not entitled");
        Ok(working.clone())
    });
    let admin = crate::Workspace::create_with(
        &data_dir(&root.path().join("ada")),
        "workspace-id".into(),
        "Acme",
        crate::Profile {
            name: "Ada",
            is_bot: false,
        },
        test_bucket_config(),
        rejects_bad_keys,
        crate::Networking::Offline,
    )
    .await
    .unwrap();
    let bad = StorageUpdate {
        region: "us-east-1",
        access_key_id: "BADKEY",
        secret_access_key: "x",
    };
    assert!(admin.update_storage(bad).await.is_err());
    assert_eq!(
        admin.storage_settings().unwrap().access_key_id,
        test_bucket_config().access_key_id
    );
}
