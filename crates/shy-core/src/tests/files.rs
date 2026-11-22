//! Sharing files and profile pictures between members.

use super::support::*;
use crate::{
    Workspace,
    files::MAX_FILE_BYTES,
    workspace::{FileSource, NewFile},
};

fn bytes_file(name: &str, content: &[u8]) -> NewFile {
    NewFile {
        name: name.into(),
        media_type: "application/octet-stream".into(),
        source: FileSource::Bytes(content.to_vec()),
    }
}

/// More than one piece, so piece order and the last piece both matter.
fn five_megabytes() -> Vec<u8> {
    (0..5 * 1024 * 1024).map(|index| (index % 253) as u8).collect()
}

fn file_ids(member: &Workspace) -> Vec<String> {
    member.files().unwrap().into_iter().map(|file| file.id).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_shared_in_a_channel_opens_for_another_member() {
    let (root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let content = five_megabytes();
    let message = admin
        .send_files(
            &general,
            "the slides",
            None,
            vec![bytes_file("deck.pdf", &content)],
        )
        .await
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let seen = bob.messages(&general, None, 50).unwrap();
    let shared = seen.iter().find(|candidate| candidate.id == message).unwrap();
    assert_eq!(shared.text, "the slides");
    assert_eq!(shared.files.len(), 1);
    let file = &shared.files[0];
    assert_eq!(
        (file.name.as_str(), file.size),
        ("deck.pdf", content.len() as u64)
    );
    assert_eq!(file.author, admin.my_id());
    assert!(!file.can_delete, "only its author or the admin may");
    assert_eq!(bob.file_bytes(&file.id).await.unwrap(), content);

    let saved = root.path().join("saved-deck.pdf");
    bob.save_file(&file.id, &saved).await.unwrap();
    assert_eq!(std::fs::read(&saved).unwrap(), content);
    assert_eq!(file_ids(&bob), std::slice::from_ref(&file.id));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_from_disk_can_be_sent_without_a_caption() {
    let (root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let path = root.path().join("notes.txt");
    std::fs::write(&path, "remember the milk").unwrap();
    let message = admin
        .send_files(
            &general,
            "  ",
            None,
            vec![NewFile {
                name: "notes.txt".into(),
                media_type: "text/plain".into(),
                source: FileSource::Path(path),
            }],
        )
        .await
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let shared = bob.messages(&general, None, 50).unwrap();
    let shared = shared.iter().find(|candidate| candidate.id == message).unwrap();
    assert_eq!(shared.text, "");
    assert_eq!(shared.files[0].media_type, "text/plain");
    assert_eq!(
        bob.file_bytes(&shared.files[0].id).await.unwrap(),
        b"remember the milk"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn files_over_the_limit_are_refused_before_anything_is_sent() {
    let (root, bucket, admin, _bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let path = root.path().join("huge.iso");
    std::fs::File::create(&path)
        .unwrap()
        .set_len(MAX_FILE_BYTES + 1)
        .unwrap();
    let next_seq = admin.state().store.next_seq(&admin.me()).unwrap();

    let refused = admin
        .send_files(
            &general,
            "",
            None,
            vec![NewFile {
                name: "huge.iso".into(),
                media_type: String::new(),
                source: FileSource::Path(path),
            }],
        )
        .await;
    assert!(refused.unwrap_err().to_string().contains("larger than 150 MB"));
    assert_eq!(admin.state().store.next_seq(&admin.me()).unwrap(), next_seq);
    assert!(bucket.blob_ids(&admin.me()).await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_message_takes_its_files_out_of_the_bucket() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let message = bob
        .send_files(
            &general,
            "two files",
            None,
            vec![bytes_file("a", b"first"), bytes_file("b", b"second")],
        )
        .await
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    assert_eq!(bucket.blob_ids(&bob.me()).await.unwrap().len(), 2);
    let file = admin.files().unwrap()[0].id.clone();
    admin.file_bytes(&file).await.unwrap();

    // The admin deletes Bob's message: the admin's device removes the pieces.
    admin.delete_message(&message).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(bucket.blob_ids(&bob.me()).await.unwrap().is_empty());
    for member in [&admin, &bob] {
        assert!(file_ids(member).is_empty());
        assert!(member.file_bytes(&file).await.is_err(), "a cached copy goes too");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_deleted_on_its_own_leaves_its_message() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let message = bob
        .send_files(
            &general,
            "keep this text",
            None,
            vec![bytes_file("draft.txt", b"v1")],
        )
        .await
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    let file = bob.files().unwrap()[0].clone();
    assert!(file.can_delete);

    bob.delete_file(&file.id).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(bucket.blob_ids(&bob.me()).await.unwrap().is_empty());
    let seen = admin.messages(&general, None, 50).unwrap();
    let kept = seen.iter().find(|candidate| candidate.id == message).unwrap();
    assert_eq!(kept.text, "keep this text");
    assert!(kept.files.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_in_a_private_channel_reaches_only_its_members() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    bob.send_files(&channel, "", None, vec![bytes_file("plan.md", b"SECRET PLAN")])
        .await
        .unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    let shared = carol.files().unwrap();
    assert_eq!(shared.len(), 1);
    assert_eq!(shared[0].place, channel);
    assert_eq!(carol.file_bytes(&shared[0].id).await.unwrap(), b"SECRET PLAN");
    assert!(file_ids(&admin).is_empty(), "the admin isn't in the channel");
    assert!(admin.file_bytes(&shared[0].id).await.is_err());

    // Deleting the channel removes its files everywhere.
    bob.delete_channel(&channel).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    assert!(file_ids(&carol).is_empty());
    assert!(bucket.blob_ids(&bob.me()).await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_picture_reaches_others_and_the_old_one_leaves_the_bucket() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let first = [b"\x89PNG\r\n\x1a\n".as_slice(), b"first"].concat();
    let second = [b"\x89PNG\r\n\x1a\n".as_slice(), b"second"].concat();
    bob.set_profile_picture(Some(first)).await.unwrap();
    sync_all(&[&admin, &bob]).await;
    bob.set_profile_picture(Some(second.clone())).await.unwrap();
    sync_all(&[&admin, &bob]).await;

    let shown = admin.members().unwrap();
    assert!(
        shown
            .iter()
            .any(|member| member.id == bob.my_id() && member.picture.is_some())
    );
    assert_eq!(admin.profile_picture(&bob.me()).await.unwrap(), Some(second));
    assert_eq!(bucket.blob_ids(&bob.me()).await.unwrap().len(), 1);

    bob.set_profile_picture(None).await.unwrap();
    sync_all(&[&admin, &bob]).await;
    assert_eq!(admin.profile_picture(&bob.me()).await.unwrap(), None);
    assert!(bucket.blob_ids(&bob.me()).await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn only_small_png_jpeg_or_webp_pictures_are_shared() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    assert!(
        admin
            .set_profile_picture(Some(b"<svg onload=alert(1)>".to_vec()))
            .await
            .is_err()
    );
    let huge = [b"\x89PNG\r\n\x1a\n".as_slice(), &vec![0; 600 * 1024]].concat();
    assert!(admin.set_profile_picture(Some(huge)).await.is_err());
}
