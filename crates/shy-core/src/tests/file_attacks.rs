//! Attacks on shared files and pictures, from members and from anyone with
//! the bucket keys.

use super::support::*;
use crate::{
    Workspace,
    files::{SharedFile, blob},
    ops::{Op, new_owned_id},
    private::{ids::new_private_message_id, ops::PrivateOp},
    workspace::{FileSource, NewFile},
};

fn bytes_file(content: &[u8]) -> NewFile {
    NewFile {
        name: "plan.txt".into(),
        media_type: "text/plain".into(),
        source: FileSource::Bytes(content.to_vec()),
    }
}

/// A file op as `author` could write it, with a blob that really is in the
/// bucket under their prefix.
async fn forged_file(
    author: &Workspace,
    bucket: &crate::transport::bucket::Bucket,
    id: String,
    place: &str,
) -> Op {
    let content = b"forged";
    let mut pieces = Vec::new();
    let blob = blob::seal(&mut &content[..], content.len() as u64, |_, _, piece| {
        pieces.push(piece.to_vec());
        Ok(())
    })
    .unwrap();
    for (index, piece) in pieces.into_iter().enumerate() {
        bucket
            .put_file_chunk(&author.me(), &blob.id, index as u32, piece)
            .await
            .unwrap();
    }
    Op::File {
        file: SharedFile {
            id,
            place: place.to_owned(),
            message: None,
            name: "forged.txt".into(),
            media_type: "text/plain".into(),
            blob,
        },
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_piece_swapped_in_the_bucket_never_opens() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    admin
        .send_files(&general, "", None, vec![bytes_file(b"pay Alice 10")])
        .await
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    let file = bob.files().unwrap()[0].clone();
    let stored = bob.state().store.file(&file.id).unwrap().unwrap();

    // Bob holds the bucket keys: he puts other sealed words where Ada's
    // piece was. (Resealing with the file's own key fails the same way; see
    // `files::blob`.)
    let mut forged = Vec::new();
    let replacement = b"pay Bob 9999";
    blob::seal(&mut &replacement[..], replacement.len() as u64, |_, _, piece| {
        forged = piece.to_vec();
        Ok(())
    })
    .unwrap();
    bucket
        .put_file_chunk(&admin.me(), &stored.blob.id, 0, forged)
        .await
        .unwrap();

    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &carol]).await;
    let refused = carol.file_bytes(&file.id).await.unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("doesn't match what its author signed")
    );
    assert!(
        carol.file_bytes(&file.id).await.is_err(),
        "the bad piece wasn't cached"
    );
    assert_eq!(admin.file_bytes(&file.id).await.unwrap(), b"pay Alice 10");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_claiming_someone_elses_id_is_skipped() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let op = forged_file(&bob, &bucket, new_owned_id(&admin.me()), &general).await;
    let mut forger = Forger::new(&bob);
    admin.receive(forger.seal(op)).unwrap();
    admin.sync().await;
    assert!(admin.files().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_file_reference_is_skipped_and_later_ops_still_apply() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let Op::File { mut file } = forged_file(&bob, &bucket, new_owned_id(&bob.me()), &general).await else {
        unreachable!()
    };
    file.blob.chunk_hashes.push([0; 32]);
    let mut forger = Forger::new(&bob);
    admin.receive(forger.seal(Op::File { file })).unwrap();
    let after = Op::Message {
        id: new_owned_id(&bob.me()),
        channel: general.clone(),
        text: "after the bad file".into(),
        reply_to: None,
    };
    admin.receive(forger.seal(after)).unwrap();
    admin.sync().await;

    assert!(admin.files().unwrap().is_empty());
    assert!(
        admin
            .messages(&general, None, 50)
            .unwrap()
            .iter()
            .any(|message| message.text == "after the bad file")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hostile_file_name_is_cleaned_before_anyone_sees_it() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let Op::File { mut file } = forged_file(&bob, &bucket, new_owned_id(&bob.me()), &general).await else {
        unreachable!()
    };
    file.name = "../../.config/autostart/evil\u{202e}txt.desktop".into();
    file.media_type = "text/html; <script>".into();
    let mut forger = Forger::new(&bob);
    admin.receive(forger.seal(Op::File { file })).unwrap();
    admin.sync().await;

    let shown = &admin.files().unwrap()[0];
    assert_eq!(shown.name, "_.._.config_autostart_eviltxt.desktop");
    assert_eq!(shown.media_type, "application/octet-stream");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ordinary_file_op_cant_reach_into_a_private_channel() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let channel = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    // Ada isn't a member, but knows the channel's id from Carol's screen.
    let op = forged_file(&admin, &bucket, new_owned_id(&admin.me()), &channel).await;
    let mut forger = Forger::new(&admin);
    carol.receive(forger.seal(op)).unwrap();
    carol.sync().await;
    assert!(carol.files().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_oversized_picture_is_skipped() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let content = vec![0u8; 600 * 1024];
    let blob = blob::seal(&mut content.as_slice(), content.len() as u64, |_, _, _| Ok(())).unwrap();
    let mut forger = Forger::new(&bob);
    admin
        .receive(forger.seal(Op::ProfilePicture { picture: Some(blob) }))
        .unwrap();
    admin.sync().await;
    assert!(
        !admin
            .members()
            .unwrap()
            .iter()
            .any(|member| member.id == bob.my_id() && member.picture.is_some())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_who_neither_wrote_nor_deleted_a_file_leaves_its_pieces_alone() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    let message = bob
        .send_files(&general, "", None, vec![bytes_file(b"notes")])
        .await
        .unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    let removals_before = carol.state().store.blob_removals().unwrap();
    assert!(removals_before.is_empty());

    // Carol hears of Bob's delete before Bob's device syncs: hers forgets
    // its copy but leaves the bucket to him.
    bob.delete_message(&message).unwrap();
    let deleted = bob.state().store.last_seq(&bob.me()).unwrap();
    for envelope in bob.state().store.envelopes_after(&bob.me(), deleted - 1).unwrap() {
        carol.receive(envelope).unwrap();
    }
    let pending = carol.state().store.blob_removals().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].requested_by, bob.me().as_bytes());
    carol.sync().await;
    assert!(carol.state().store.blob_removals().unwrap().is_empty());
    assert_eq!(bucket.blob_ids(&bob.me()).await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_sealed_into_one_private_channel_cant_name_another() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let plans = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    let secrets = bob.create_private_channel("secrets", &[carol.my_id()]).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    // Carol seals, under the plans key, a file that claims to live in secrets.
    let plans_key = carol
        .state()
        .store
        .channel_keys(Some(&plans))
        .unwrap()
        .pop()
        .unwrap();
    let op = forged_file(
        &carol,
        &bucket,
        new_private_message_id(&plans, &carol.me()),
        &secrets,
    )
    .await;
    let mut forger = Forger::new(&carol);
    bob.receive(forger.seal_private(&plans_key, PrivateOp::Content(op)))
        .unwrap();
    bob.sync().await;
    assert!(bob.files().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forged_huge_size_is_refused_without_reserving_memory() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let Op::File { mut file } = forged_file(&bob, &bucket, new_owned_id(&bob.me()), &general).await else {
        unreachable!()
    };
    // About 117 GB claimed with real-looking piece hashes, nothing uploaded.
    file.blob.chunk_size = 16 * 1024 * 1024;
    file.blob.chunk_hashes = vec![[0; 32]; 7_000];
    file.blob.size = 7_000 * 16 * 1024 * 1024;
    let mut forger = Forger::new(&bob);
    admin.receive(forger.seal(Op::File { file })).unwrap();
    admin.sync().await;
    assert!(admin.files().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_too_large_to_show_is_refused_in_memory_but_still_saves() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let Op::File { mut file } = forged_file(&bob, &bucket, new_owned_id(&bob.me()), &general).await else {
        unreachable!()
    };
    // Within what receivers accept, but larger than anything opened whole.
    file.blob.chunk_size = 16 * 1024 * 1024;
    file.blob.chunk_hashes = vec![[0; 32]; 5];
    file.blob.size = 5 * 16 * 1024 * 1024;
    let id = file.id.clone();
    let mut forger = Forger::new(&bob);
    admin.receive(forger.seal(Op::File { file })).unwrap();
    admin.sync().await;
    let refused = admin.file_bytes(&id).await.unwrap_err();
    assert!(refused.to_string().contains("too large to open here"));
    // Saving streams it, and fails cleanly on the missing pieces.
    let destination = root.path().join("big.bin");
    assert!(admin.save_file(&id, &destination).await.is_err());
    assert!(!destination.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_cant_be_attached_to_someone_elses_message() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let invoice = admin
        .send_message(&general, "the official invoice is below", None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let Op::File { mut file } = forged_file(&bob, &bucket, new_owned_id(&bob.me()), &general).await else {
        unreachable!()
    };
    file.message = Some(invoice.clone());
    let mut forger = Forger::new(&bob);
    admin.receive(forger.seal(Op::File { file })).unwrap();
    admin.sync().await;

    let shown = admin.messages(&general, None, 50).unwrap();
    let kept = shown.iter().find(|message| message.id == invoice).unwrap();
    assert!(kept.files.is_empty());
    assert!(admin.files().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn in_a_private_channel_a_file_cant_be_attached_to_another_members_message() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let plans = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    let bobs = bob
        .send_message(&plans, "the final plan is attached", None)
        .unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    let plans_key = carol
        .state()
        .store
        .channel_keys(Some(&plans))
        .unwrap()
        .pop()
        .unwrap();
    let Op::File { mut file } = forged_file(
        &carol,
        &bucket,
        new_private_message_id(&plans, &carol.me()),
        &plans,
    )
    .await
    else {
        unreachable!()
    };
    file.message = Some(bobs.clone());
    let mut forger = Forger::new(&carol);
    bob.receive(forger.seal_private(&plans_key, PrivateOp::Content(Op::File { file })))
        .unwrap();
    bob.sync().await;
    let shown = bob.messages(&plans, None, 50).unwrap();
    assert!(
        shown
            .iter()
            .find(|message| message.id == bobs)
            .unwrap()
            .files
            .is_empty()
    );
    assert!(bob.files().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_borrowed_blob_id_cant_spoil_someone_elses_cached_picture() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let picture = [b"\x89PNG\r\n\x1a\n".as_slice(), b"ada"].concat();
    admin.set_profile_picture(Some(picture.clone())).await.unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    let adas_blob = carol
        .state()
        .store
        .profile_picture(admin.me().as_bytes())
        .unwrap()
        .unwrap();

    // Bob reuses Ada's blob id for junk under his own prefix, signed by him.
    let junk = b"\x89PNG\r\n\x1a\njunk".to_vec();
    let mut pieces = Vec::new();
    let mut his = blob::seal(&mut junk.as_slice(), junk.len() as u64, |_, _, piece| {
        pieces.push(piece.to_vec());
        Ok(())
    })
    .unwrap();
    his.id = adas_blob.id.clone();
    bucket
        .put_file_chunk(&bob.me(), &his.id, 0, pieces.remove(0))
        .await
        .unwrap();
    let mut forger = Forger::new(&bob);
    carol
        .receive(forger.seal(Op::ProfilePicture { picture: Some(his) }))
        .unwrap();
    carol.sync().await;

    // Carol's device fetches and caches Bob's piece (its hash matches what
    // he signed), but it never opens and never stands in for Ada's.
    assert!(carol.profile_picture(&bob.me()).await.is_err());
    assert_eq!(carol.profile_picture(&admin.me()).await.unwrap(), Some(picture));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_delete_heard_before_its_file_still_removes_the_file() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    let before = bob.state().store.last_seq(&bob.me()).unwrap();
    let message = bob
        .send_files(&general, "", None, vec![bytes_file(b"oops")])
        .await
        .unwrap();
    // Ada sees it and deletes it; Carol hears Ada's delete first.
    for envelope in bob.state().store.envelopes_after(&bob.me(), before).unwrap() {
        admin.receive(envelope).unwrap();
    }
    let admin_before = admin.state().store.last_seq(&admin.me()).unwrap();
    admin.delete_message(&message).unwrap();
    for envelope in admin
        .state()
        .store
        .envelopes_after(&admin.me(), admin_before)
        .unwrap()
    {
        carol.receive(envelope).unwrap();
    }
    for envelope in bob.state().store.envelopes_after(&bob.me(), before).unwrap() {
        carol.receive(envelope).unwrap();
    }
    assert!(carol.files().unwrap().is_empty());
    assert!(
        carol
            .messages(&general, None, 50)
            .unwrap()
            .iter()
            .all(|shown| shown.id != message)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pictures_settle_on_the_newest_in_any_order_and_applied_twice() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let first = [b"\x89PNG\r\n\x1a\n".as_slice(), b"1"].concat();
    let second = [b"\x89PNG\r\n\x1a\n".as_slice(), b"2"].concat();
    let before = bob.state().store.last_seq(&bob.me()).unwrap();
    bob.set_profile_picture(Some(first)).await.unwrap();
    bob.set_profile_picture(Some(second.clone())).await.unwrap();
    let mut envelopes = bob.state().store.envelopes_after(&bob.me(), before).unwrap();
    // Out of order, the second first: the gap-free log parks it until the
    // first arrives, then both apply in order; then again, as a replay.
    envelopes.reverse();
    for envelope in envelopes.iter().chain(envelopes.iter()) {
        admin.receive(envelope.clone()).unwrap();
    }
    admin.sync().await;
    assert_eq!(admin.profile_picture(&bob.me()).await.unwrap(), Some(second));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_damaged_cached_piece_is_fetched_again() {
    let (root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    admin
        .send_files(&general, "", None, vec![bytes_file(b"the real plan")])
        .await
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    let file = bob.files().unwrap()[0].id.clone();
    assert_eq!(bob.file_bytes(&file).await.unwrap(), b"the real plan");

    let blob = bob.state().store.file(&file).unwrap().unwrap().blob;
    let piece = root
        .path()
        .join("bob")
        .join("file-cache")
        .join(format!("{}-{}", admin.my_id(), blob.id))
        .join("0");
    std::fs::write(&piece, b"damaged on disk").unwrap();
    assert_eq!(bob.file_bytes(&file).await.unwrap(), b"the real plan");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_id_used_twice_keeps_the_first_file_even_when_replayed() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let id = new_owned_id(&bob.me());
    let Op::File { file: mut first } = forged_file(&bob, &bucket, id.clone(), &general).await else {
        unreachable!()
    };
    first.name = "first.txt".into();
    let mut second = first.clone();
    second.name = "second.txt".into();
    let mut forger = Forger::new(&bob);
    let envelopes = [
        forger.seal(Op::File { file: first }),
        forger.seal(Op::File { file: second }),
    ];
    for envelope in envelopes.iter().chain(envelopes.iter()) {
        admin.receive(envelope.clone()).unwrap();
    }
    admin.sync().await;
    let shown = admin.files().unwrap();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].name, "first.txt");
}

#[tokio::test(flavor = "multi_thread")]
async fn someone_elses_file_cant_be_renamed() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    admin
        .send_files(&general, "", None, vec![bytes_file(b"invoice")])
        .await
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    let file = bob.files().unwrap()[0].id.clone();

    let mut forger = Forger::new(&bob);
    admin
        .receive(forger.seal(Op::FileRename {
            id: file.clone(),
            name: "paid-in-full.pdf".into(),
        }))
        .unwrap();
    admin.sync().await;
    assert_eq!(admin.files().unwrap()[0].name, "plan.txt");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hostile_new_name_is_cleaned_like_any_file_name() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_files(&general, "", None, vec![bytes_file(b"notes")])
        .await
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    let file = admin.files().unwrap()[0].id.clone();
    let mut forger = Forger::new(&bob);
    admin
        .receive(forger.seal(Op::FileRename {
            id: file,
            name: "../../.ssh/authorized_keys\u{202e}gpj.exe".into(),
        }))
        .unwrap();
    admin.sync().await;
    assert_eq!(admin.files().unwrap()[0].name, "_.._.ssh_authorized_keysgpj.exe");
}

#[tokio::test(flavor = "multi_thread")]
async fn in_a_private_channel_only_the_sharer_renames() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let plans = bob.create_private_channel("plans", &[carol.my_id()]).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    bob.send_files(&plans, "", None, vec![bytes_file(b"secret")])
        .await
        .unwrap();
    sync_all(&[&admin, &bob, &carol]).await;
    let file = carol.files().unwrap()[0].id.clone();

    let plans_key = carol
        .state()
        .store
        .channel_keys(Some(&plans))
        .unwrap()
        .pop()
        .unwrap();
    let mut forger = Forger::new(&carol);
    bob.receive(forger.seal_private(
        &plans_key,
        PrivateOp::Content(Op::FileRename {
            id: file,
            name: "carol-wrote-this.txt".into(),
        }),
    ))
    .unwrap();
    bob.sync().await;
    assert_eq!(bob.files().unwrap()[0].name, "plan.txt");
}

#[tokio::test(flavor = "multi_thread")]
async fn renaming_a_deleted_file_brings_nothing_back() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_files(&general, "", None, vec![bytes_file(b"gone")])
        .await
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    let file = bob.files().unwrap()[0].id.clone();
    bob.delete_file(&file).unwrap();
    let mut forger = Forger::new(&bob);
    let rename = forger.seal(Op::FileRename {
        id: file,
        name: "back.txt".into(),
    });
    sync_all(&[&admin, &bob]).await;
    admin.receive(rename).unwrap();
    admin.sync().await;
    assert!(admin.files().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_opens_only_its_own_files() {
    let (root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let plan = admin.create_page("Plan", None).unwrap();
    let other = admin.create_page("Other", None).unwrap();
    let message = admin
        .send_files(&general, "", None, vec![bytes_file(b"from chat")])
        .await
        .unwrap();
    let in_chat = admin
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == message)
        .unwrap()
        .files[0]
        .id
        .clone();
    let in_other_page = admin
        .share_page_file(&other, bytes_file(b"from another page"))
        .await
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let saved = root.path().join("saved");
    for file in [&in_chat, &in_other_page] {
        assert!(bob.page_file_bytes(&plan, file).await.is_err(), "{file}");
        assert!(bob.save_page_file(&plan, file, &saved).await.is_err(), "{file}");
        assert!(!saved.exists());
        assert!(bob.page_files(&plan).unwrap().is_empty());
    }
    assert!(bob.page_file_bytes(&other, &in_other_page).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_file_arriving_after_its_page_was_deleted_is_dropped() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let page = admin.create_page("Plan", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    admin.delete_page(&page).unwrap();
    sync_all(&[&admin, &bob]).await;

    let late = forged_file(&bob, &bucket, new_owned_id(&bob.me()), &page).await;
    let mut forger = Forger::new(&bob);
    admin.receive(forger.seal(late)).unwrap();
    assert!(admin.page_files(&page).unwrap().is_empty());
    assert!(admin.files().unwrap().is_empty());
}
