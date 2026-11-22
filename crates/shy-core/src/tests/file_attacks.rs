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
            .any(|member| member.id == bob.my_id() && member.has_picture)
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
