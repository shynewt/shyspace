//! Restoring a lost admin from their recovery kit.

use std::{
    fmt,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use futures_lite::stream;
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, memory::InMemory, path::Path,
};

use super::support::*;
use crate::{
    Networking, Workspace,
    transport::bucket::Bucket,
    views::member_id,
    workspace::{FileSource, NewFile},
};

fn bytes_file(name: &str, content: &[u8]) -> NewFile {
    NewFile {
        name: name.into(),
        media_type: "application/octet-stream".into(),
        source: FileSource::Bytes(content.to_vec()),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_admin_is_restored_from_the_recovery_kit() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let kit = admin.recovery_kit().await.unwrap();
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let general = channel_id(&admin, "general");
    admin
        .send_message(&general, "written before the device was lost", None)
        .unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    let admin_id = admin.me();
    drop(admin);

    let new_device = data_dir(&root.path().join("ada-new-laptop"));
    let restored = Workspace::restore_with(&new_device, &kit, connector(&bucket), Networking::Offline)
        .await
        .unwrap();
    assert_eq!(restored.me(), admin_id);
    assert!(restored.is_admin());
    let texts: Vec<_> = restored
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert!(texts.contains(&"written before the device was lost".to_string()));
    let active: Vec<_> = restored
        .members()
        .unwrap()
        .into_iter()
        .filter(|m| m.active)
        .map(|m| m.name)
        .collect();
    assert!(active.contains(&"Bob".to_string()) && active.contains(&"Carol".to_string()));

    // Still the admin: it can change membership, and members follow.
    let dave = join(&restored, &root.path().join("dave"), &bucket, "Dave", false).await;
    restored
        .send_message(&general, "back on a new laptop", None)
        .unwrap();
    sync_all(&[&restored, &bob, &dave]).await;
    let bob_sees: Vec<_> = bob
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert!(bob_sees.contains(&"back on a new laptop".to_string()));
    assert!(
        bob.members()
            .unwrap()
            .iter()
            .any(|member| member.name == "Dave" && member.active)
    );
}

/// Whether anyone in `members` holds two versions of one of `author`'s ops.
fn has_fork(members: &[&Workspace], author: &iroh::PublicKey) -> bool {
    members.iter().any(|member| {
        let last = member.state().store.last_seq(author).unwrap();
        (1..=last + 5).any(|seq| member.state().store.is_forked(author, seq).unwrap())
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_never_reuses_the_admins_sequence_numbers() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let kit = admin.recovery_kit().await.unwrap();
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "before", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let admin_id = admin.me();
    drop(admin);

    let new_device = data_dir(&root.path().join("ada-new-laptop"));
    let restored = Workspace::restore_with(&new_device, &kit, connector(&bucket), Networking::Offline)
        .await
        .unwrap();
    restored.send_message(&general, "after", None).unwrap();
    sync_all(&[&restored, &bob]).await;

    assert!(!has_fork(&[&restored, &bob], &admin_id));
    let bob_sees: Vec<_> = bob
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect();
    assert!(bob_sees.contains(&"after".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_cut_short_writes_nothing_until_it_has_caught_up() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let kit = admin.recovery_kit().await.unwrap();
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "before", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let admin_id = admin.me();
    drop(admin);

    // The app quits after laying out the restored files, before catching up.
    let new_device = data_dir(&root.path().join("ada-new-laptop"));
    let cut_short =
        Workspace::restore_without_catching_up(&new_device, &kit, connector(&bucket), Networking::Offline)
            .await
            .unwrap();
    drop(cut_short);

    let reopened = Workspace::open_with(&new_device, connector(&bucket), Networking::Offline)
        .await
        .unwrap();
    assert!(
        reopened.send_message(&general, "too early", None).is_err(),
        "nothing is written before the device has its own changes back"
    );
    reopened.sync().await;
    reopened.send_message(&general, "after", None).unwrap();
    sync_all(&[&reopened, &bob]).await;
    assert!(!has_fork(&[&reopened, &bob], &admin_id));
}

/// A bucket in which listing keys under one prefix always fails, and so
/// does reading objects under another, as objects nobody can read would.
#[derive(Debug, Default)]
struct UnlistableStore {
    inner: InMemory,
    unlistable: Mutex<Option<String>>,
    unreadable: Mutex<Option<String>>,
}

impl UnlistableStore {
    fn is_unlistable(&self, prefix: Option<&Path>) -> bool {
        let unlistable = self.unlistable.lock().unwrap();
        matches!((unlistable.as_deref(), prefix), (Some(unlistable), Some(prefix)) if prefix.as_ref().starts_with(unlistable))
    }

    fn is_unreadable(&self, location: &Path) -> bool {
        let unreadable = self.unreadable.lock().unwrap();
        unreadable
            .as_deref()
            .is_some_and(|unreadable| location.as_ref().starts_with(unreadable))
    }

    fn refused() -> stream::Boxed<object_store::Result<ObjectMeta>> {
        Box::pin(stream::once(Err(object_store::Error::Generic {
            store: "unlistable",
            source: "this listing can't be read".into(),
        })))
    }
}

impl fmt::Display for UnlistableStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unlistable store")
    }
}

#[async_trait]
impl ObjectStore for UnlistableStore {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        opts: PutOptions,
    ) -> object_store::Result<PutResult> {
        self.inner.put_opts(location, payload, opts).await
    }
    async fn put_multipart_opts(
        &self,
        location: &Path,
        opts: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(location, opts).await
    }
    async fn get_opts(&self, location: &Path, options: GetOptions) -> object_store::Result<GetResult> {
        if self.is_unreadable(location) {
            return Err(object_store::Error::Generic {
                store: "unlistable",
                source: "this object can't be read".into(),
            });
        }
        self.inner.get_opts(location, options).await
    }
    fn delete_stream(
        &self,
        locations: stream::Boxed<object_store::Result<Path>>,
    ) -> stream::Boxed<object_store::Result<Path>> {
        self.inner.delete_stream(locations)
    }
    fn list(&self, prefix: Option<&Path>) -> stream::Boxed<object_store::Result<ObjectMeta>> {
        match self.is_unlistable(prefix) {
            true => Self::refused(),
            false => self.inner.list(prefix),
        }
    }
    fn list_with_offset(
        &self,
        prefix: Option<&Path>,
        offset: &Path,
    ) -> stream::Boxed<object_store::Result<ObjectMeta>> {
        match self.is_unlistable(prefix) {
            true => Self::refused(),
            false => self.inner.list_with_offset(prefix, offset),
        }
    }
    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }
    async fn copy_opts(&self, from: &Path, to: &Path, options: CopyOptions) -> object_store::Result<()> {
        self.inner.copy_opts(from, to, options).await
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn someone_elses_unreadable_log_doesnt_block_a_restore() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(UnlistableStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let kit = admin.recovery_kit().await.unwrap();
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob]).await;
    let bobs_log = format!("ops/{}", member_id(&bob.me()));
    *store.unlistable.lock().unwrap() = Some(bobs_log);
    drop(admin);

    let new_device = data_dir(&root.path().join("ada-new-laptop"));
    let restored = Workspace::restore_with(&new_device, &kit, connector(&bucket), Networking::Offline)
        .await
        .unwrap();
    restored.send_message(&general, "back", None).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_that_cant_read_its_own_log_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(UnlistableStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let kit = admin.recovery_kit().await.unwrap();
    admin.sync().await;
    let own_log = format!("ops/{}", member_id(&admin.me()));
    *store.unlistable.lock().unwrap() = Some(own_log);
    drop(admin);

    let new_device = data_dir(&root.path().join("ada-new-laptop"));
    let refused = Workspace::restore_with(&new_device, &kit, connector(&bucket), Networking::Offline).await;
    assert!(refused.is_err());
    assert!(
        !new_device.path.exists(),
        "a refused restore leaves nothing behind"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forged_or_unknown_kit_is_refused() {
    let (root, bucket, admin, _bob) = admin_and_bob().await;
    let kit = admin.recovery_kit().await.unwrap();
    let mut forged = kit.to_string();
    forged.truncate(forged.len() - 4);
    forged.push_str("AAAA");
    let attempt = Workspace::restore_with(
        &data_dir(&root.path().join("x")),
        &forged,
        connector(&bucket),
        Networking::Offline,
    )
    .await;
    assert!(attempt.is_err());

    let fresh_bucket = memory_bucket();
    let attempt = Workspace::restore_with(
        &data_dir(&root.path().join("y")),
        &kit,
        connector(&fresh_bucket),
        Networking::Offline,
    )
    .await;
    assert!(attempt.is_err(), "no snapshot in that bucket");
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_admin_has_a_recovery_kit() {
    let (_root, _bucket, _admin, bob) = admin_and_bob().await;
    assert!(bob.recovery_kit().await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rolled_back_snapshot_is_refused() {
    let (root, bucket, admin, _bob) = admin_and_bob().await;
    let kit = admin.recovery_kit().await.unwrap();
    let old_snapshot = bucket.recovery_snapshot().await.unwrap().unwrap();
    let _carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;

    // Someone with bucket access puts back the snapshot from before Carol.
    bucket.put_recovery_snapshot(old_snapshot.to_vec()).await.unwrap();
    let error = Workspace::restore_with(
        &data_dir(&root.path().join("ada-new")),
        &kit,
        connector(&bucket),
        Networking::Offline,
    )
    .await
    .err()
    .unwrap();
    assert!(format!("{error:#}").contains("changed after"), "{error:#}");
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_needs_an_empty_folder() {
    let (root, bucket, admin, _bob) = admin_and_bob().await;
    let kit = admin.recovery_kit().await.unwrap();
    let busy = root.path().join("busy");
    std::fs::create_dir_all(&busy).unwrap();
    std::fs::write(busy.join("notes.txt"), "keep me").unwrap();
    assert!(
        Workspace::restore_with(&data_dir(&busy), &kit, connector(&bucket), Networking::Offline)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(busy.join("notes.txt")).unwrap(),
        "keep me"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_commit_slots_does_not_make_a_stale_snapshot_restorable() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let kit = admin.recovery_kit().await.unwrap();
    let old_snapshot = bucket.recovery_snapshot().await.unwrap().unwrap();
    let snapshot_epoch = admin.state().mls.epoch().unwrap();
    join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    join(&admin, &root.path().join("dave"), &bucket, "Dave", false).await;
    sync_all(&[&admin, &bob]).await;

    // An attacker restores the old snapshot and deletes the next commit slot.
    bucket.put_recovery_snapshot(old_snapshot.to_vec()).await.unwrap();
    bucket.delete_commit(snapshot_epoch).await.unwrap();
    let attempt = |name: &str| {
        let dir = data_dir(&root.path().join(name));
        let bucket = bucket.clone();
        let kit = kit.to_string();
        async move { Workspace::restore_with(&dir, &kit, connector(&bucket), Networking::Offline).await }
    };
    assert!(
        attempt("try-1").await.is_err(),
        "the later commit still gives it away"
    );

    // Deleting every later slot doesn't help either: members put them back.
    bucket.delete_commit(snapshot_epoch + 1).await.unwrap();
    bob.sync().await;
    assert!(
        attempt("try-2").await.is_err(),
        "Bob re-uploaded the commits he had applied"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_files_and_pictures_survive_a_restore() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let picture = [b"\x89PNG\r\n\x1a\n".as_slice(), b"ada"].concat();
    admin.set_profile_picture(Some(picture.clone())).await.unwrap();
    bob.set_profile_picture(Some(picture.clone())).await.unwrap();
    admin
        .send_files(&general, "", None, vec![bytes_file("plan.txt", b"the plan")])
        .await
        .unwrap();
    bob.send_files(&general, "", None, vec![bytes_file("notes.txt", b"bob's notes")])
        .await
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    // Taken now, after the files and pictures exist.
    let kit = admin.recovery_kit().await.unwrap();
    let admin_id = admin.me();
    let pieces_before = (
        bucket.blob_ids(&admin_id).await.unwrap().len(),
        bucket.blob_ids(&bob.me()).await.unwrap().len(),
    );
    drop(admin);

    let new_device = data_dir(&root.path().join("ada-new-laptop"));
    let restored = Workspace::restore_with(&new_device, &kit, connector(&bucket), Networking::Offline)
        .await
        .unwrap();
    // Twice, so removals the replay may have queued have run.
    restored.sync().await;
    restored.sync().await;

    let mut contents = Vec::new();
    for file in restored.files().unwrap() {
        contents.push(restored.file_bytes(&file.id).await.unwrap());
    }
    contents.sort();
    assert_eq!(contents, [b"bob's notes".to_vec(), b"the plan".to_vec()]);
    assert_eq!(
        restored.profile_picture(&admin_id).await.unwrap(),
        Some(picture.clone())
    );
    assert_eq!(restored.profile_picture(&bob.me()).await.unwrap(), Some(picture));
    let pieces_after = (
        bucket.blob_ids(&admin_id).await.unwrap().len(),
        bucket.blob_ids(&bob.me()).await.unwrap().len(),
    );
    assert_eq!(pieces_after, pieces_before, "nothing left the bucket");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_admin_sees_how_close_the_snapshot_is_to_what_restore_reads() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    assert_eq!(
        admin.storage_settings().unwrap().recovery_snapshot_bytes,
        None,
        "no kit yet"
    );

    admin.recovery_kit().await.unwrap();

    let settings = admin.storage_settings().unwrap();
    assert!(settings.recovery_snapshot_bytes.unwrap() > 0);
    assert_eq!(settings.recovery_snapshot_limit, 32 * 1024 * 1024);
    assert_eq!(
        bob.storage_settings().unwrap().recovery_snapshot_bytes,
        None,
        "members have none"
    );
}

/// The sidebar is personal, but the admin's own sections come back as the
/// snapshot had them, with their channels in them, not empty.
#[tokio::test(flavor = "multi_thread")]
async fn a_restored_admin_gets_their_sidebar_sections_with_their_channels() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let section = admin.create_section("Daily").unwrap();
    admin.move_channel(&general, Some(&section)).unwrap();
    let kit = admin.recovery_kit().await.unwrap();
    sync_all(&[&admin, &bob]).await;
    drop(admin);

    let new_device = data_dir(&root.path().join("ada-new-laptop"));
    let restored = Workspace::restore_with(&new_device, &kit, connector(&bucket), Networking::Offline)
        .await
        .unwrap();

    let daily = restored
        .sidebar_sections()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == section)
        .expect("the section came back");
    assert_eq!(daily.channels, [general]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_waits_for_an_own_change_it_cant_apply_yet() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let kit = admin.recovery_kit().await.unwrap();
    // The lost device ran a newer version: its last change is in a newer format.
    let (epoch, key) = current_key(&admin);
    let seq = admin.state().store.next_seq(&admin.me()).unwrap();
    let newer = postcard::to_stdvec(&(crate::crypto::OP_FORMAT + 1, "a new kind of change")).unwrap();
    let envelope = crate::crypto::seal_bytes(&admin.inner.identity, &key, epoch, seq, newer).unwrap();
    admin.state().store.insert_applied(&envelope, false).unwrap();
    admin.sync().await;
    bob.sync().await;
    let admin_id = admin.me();
    drop(admin);

    let new_device = data_dir(&root.path().join("ada-new-laptop"));
    let refused = Workspace::restore_with(&new_device, &kit, connector(&bucket), Networking::Offline).await;
    let error = format!(
        "{:#}",
        refused.err().expect("the restore can't continue the log yet")
    );
    assert!(error.contains("update"), "{error}");
    assert!(!new_device.path.exists());
    assert!(!has_fork(&[&bob], &admin_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_tried_again_after_failing_online_doesnt_fork() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(UnlistableStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&admin, &root.path().join("bob"), &bucket, "Bob", false).await;
    let kit = admin.recovery_kit().await.unwrap();
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob]).await;
    let admin_id = admin.me();
    *store.unlistable.lock().unwrap() = Some(format!("ops/{}", member_id(&admin_id)));
    drop(admin);

    let new_device = data_dir(&root.path().join("ada-new-laptop"));
    assert!(
        Workspace::restore_with(&new_device, &kit, connector(&bucket), Networking::Online)
            .await
            .is_err()
    );
    *store.unlistable.lock().unwrap() = None;
    let restored = Workspace::restore_with(&new_device, &kit, connector(&bucket), Networking::Online)
        .await
        .unwrap();
    restored.send_message(&general, "back", None).unwrap();
    sync_all(&[&restored, &bob]).await;
    assert!(!has_fork(&[&restored, &bob], &admin_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_never_gives_up_on_its_own_log() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(UnlistableStore::default());
    let bucket = Bucket::without_signing(store.clone());
    let admin = create_admin(&root.path().join("ada"), &bucket).await;
    let kit = admin.recovery_kit().await.unwrap();
    let general = channel_id(&admin, "general");
    admin.sync().await;
    let admin_id = admin.me();
    *store.unreadable.lock().unwrap() = Some(format!("ops/{}", member_id(&admin_id)));
    drop(admin);

    let new_device = data_dir(&root.path().join("ada-new-laptop"));
    let restored =
        Workspace::restore_without_catching_up(&new_device, &kit, connector(&bucket), Networking::Offline)
            .await
            .unwrap();
    for _ in 0..5 {
        restored.sync().await;
    }
    assert!(restored.state().store.is_catching_up().unwrap());
    assert!(restored.send_message(&general, "too early", None).is_err());
    assert!(
        restored.status().error.is_some(),
        "the unread log shows as an error"
    );
}
