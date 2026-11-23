//! Forked logs (#109): an author who signs two versions of one op can't show
//! each to different people, and nobody else can make it look as if they did.

use super::support::*;
use crate::{
    Workspace,
    ops::{Envelope, Op, new_owned_id},
    transport::bucket::{Bucket, fork_proof_path},
};

fn message(author: &Workspace, channel: &str, text: &str) -> Op {
    Op::Message {
        id: new_owned_id(&author.me()),
        channel: channel.to_owned(),
        text: text.into(),
        reply_to: None,
    }
}

fn texts(member: &Workspace, channel: &str) -> Vec<String> {
    member
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect()
}

/// Admin, Bob and Carol, plus two versions of Bob's next op and the op
/// after it, none of them in Bob's own log.
struct Fork {
    _root: tempfile::TempDir,
    bucket: Bucket,
    admin: Workspace,
    bob: Workspace,
    carol: Workspace,
    general: String,
    for_admin: Envelope,
    for_carol: Envelope,
    after: Envelope,
}

async fn fork() -> Fork {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    let for_admin = Forger::new(&bob).seal(message(&bob, &general, "version for the admin"));
    let mut forger = Forger::new(&bob);
    let for_carol = forger.seal(message(&bob, &general, "version for carol"));
    let after = forger.seal(message(&bob, &general, "after"));
    Fork {
        _root: root,
        bucket,
        admin,
        bob,
        carol,
        general,
        for_admin,
        for_carol,
        after,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn two_versions_of_one_op_count_on_no_device() {
    let fork = fork().await;
    let (admin, carol, general) = (&fork.admin, &fork.carol, &fork.general);
    admin.receive(fork.for_admin.clone()).unwrap();
    carol.receive(fork.for_carol.clone()).unwrap();
    for member in [admin, carol] {
        member.receive(fork.after.clone()).unwrap();
    }
    assert_eq!(texts(admin, general), ["version for the admin", "after"]);
    assert_eq!(texts(carol, general), ["version for carol", "after"]);

    // The admin's device also reads the other version, say from the bucket,
    // and the proof reaches Carol through it.
    admin.receive(fork.for_carol.clone()).unwrap();
    assert_eq!(texts(admin, general), ["after"]);
    sync_all(&[admin, carol]).await;
    assert_eq!(texts(carol, general), ["after"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fork_learned_before_the_op_keeps_it_from_counting() {
    let fork = fork().await;
    let (admin, carol, general) = (&fork.admin, &fork.carol, &fork.general);
    admin.receive(fork.for_admin.clone()).unwrap();
    admin.receive(fork.for_carol.clone()).unwrap();
    sync_all(&[admin, carol]).await;

    carol.receive(fork.for_carol.clone()).unwrap();
    carol.receive(fork.after.clone()).unwrap();
    assert_eq!(
        texts(carol, general),
        ["after"],
        "and the author's next op isn't held up"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_copy_encrypted_again_frames_nobody() {
    let fork = fork().await;
    let (admin, carol, general) = (&fork.admin, &fork.carol, &fork.general);
    admin.receive(fork.for_admin.clone()).unwrap();
    let (_, epoch_key) = current_key(admin);
    for padding in [0, 100] {
        let copy = encrypted_again(&fork.for_admin, &epoch_key, padding);
        assert!(
            admin.receive(copy).is_err(),
            "a copy that proves nothing is refused"
        );
    }
    assert_eq!(texts(admin, general), ["version for the admin"]);

    // Nor does a "proof" made of two copies, put in the bucket by hand, and
    // it's fetched only once.
    carol.receive(fork.for_admin.clone()).unwrap();
    let copies = (
        fork.for_admin.clone(),
        encrypted_again(&fork.for_admin, &epoch_key, 100),
    );
    let proof = postcard::to_stdvec(&copies).unwrap();
    let path = fork_proof_path(&fork.bob.me(), fork.for_admin.seq, &proof);
    fork.bucket
        .put_fork_proof(&fork.bob.me(), fork.for_admin.seq, proof)
        .await
        .unwrap();
    sync_all(&[admin, carol]).await;
    assert_eq!(texts(carol, general), ["version for the admin"]);
    assert!(!carol.state().store.should_fetch_fork_proof(&path).unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_proof_about_another_op_proves_nothing() {
    let fork = fork().await;
    let (admin, carol, general) = (&fork.admin, &fork.carol, &fork.general);
    for member in [admin, carol] {
        member.receive(fork.for_admin.clone()).unwrap();
        member.receive(fork.after.clone()).unwrap();
    }

    // Two real but different ops of Bob's, filed as if they shared a number.
    let mismatched = (fork.for_admin.clone(), fork.after.clone());
    fork.bucket
        .put_fork_proof(
            &fork.bob.me(),
            fork.for_admin.seq,
            postcard::to_stdvec(&mismatched).unwrap(),
        )
        .await
        .unwrap();
    sync_all(&[admin, carol]).await;
    assert_eq!(texts(carol, general), ["version for the admin", "after"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_version_too_large_to_count_still_proves_the_fork() {
    let fork = fork().await;
    let (admin, carol, general) = (&fork.admin, &fork.carol, &fork.general);
    let huge = Forger::new(&fork.bob).seal(message(&fork.bob, general, &"x".repeat(9 << 20)));
    admin.receive(fork.for_admin.clone()).unwrap();
    carol.receive(huge.clone()).unwrap();
    assert_eq!(texts(admin, general), ["version for the admin"]);

    admin.receive(huge).unwrap();
    sync_all(&[admin, carol]).await;
    for member in [admin, carol] {
        assert!(texts(member, general).is_empty());
        assert!(
            member
                .state()
                .store
                .is_forked(&fork.bob.me(), fork.for_admin.seq)
                .unwrap()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_proof_for_an_epoch_far_ahead_is_refused_and_not_fetched_again() {
    let fork = fork().await;
    let (carol, general) = (&fork.carol, &fork.general);
    carol.receive(fork.for_admin.clone()).unwrap();
    let mut far_ahead = fork.for_carol.clone();
    far_ahead.epoch += 1_000;
    let proof = postcard::to_stdvec(&(fork.for_admin.clone(), far_ahead)).unwrap();
    let path = fork_proof_path(&fork.bob.me(), fork.for_admin.seq, &proof);
    fork.bucket
        .put_fork_proof(&fork.bob.me(), fork.for_admin.seq, proof)
        .await
        .unwrap();

    sync_all(&[carol]).await;
    assert_eq!(texts(carol, general), ["version for the admin"]);
    assert!(!carol.state().store.should_fetch_fork_proof(&path).unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn two_versions_waiting_for_their_turn_still_prove_the_fork() {
    let fork = fork().await;
    let (admin, general) = (&fork.admin, &fork.general);
    let mut forger = Forger::new(&fork.bob);
    let before = forger.seal(message(&fork.bob, general, "before"));
    let one = forger.seal(message(&fork.bob, general, "one version"));
    let mut other_forger = Forger::new(&fork.bob);
    other_forger.next_seq += 1;
    let other = other_forger.seal(message(&fork.bob, general, "other version"));

    admin.receive(one).unwrap();
    admin.receive(other).unwrap();
    admin.receive(before).unwrap();
    sync_all(&[admin]).await;
    assert_eq!(texts(admin, general), ["before"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_overwritten_proof_is_put_back_and_still_learned() {
    let fork = fork().await;
    let (admin, carol, general) = (&fork.admin, &fork.carol, &fork.general);
    admin.receive(fork.for_admin.clone()).unwrap();
    admin.receive(fork.for_carol.clone()).unwrap();
    carol.receive(fork.for_carol.clone()).unwrap();
    sync_all(&[admin]).await;

    // Bob overwrites the proof with junk before Carol fetches it.
    let proof_path = only_fork_proof_in(&fork.bucket).await;
    fork.bucket.put_raw(&proof_path, b"junk".to_vec()).await.unwrap();
    sync_all(&[carol]).await;
    assert_eq!(texts(carol, general), ["version for carol"]);

    // The admin finds it overwritten and puts it back, and Carol looks at
    // the new version.
    sync_all(&[admin, carol]).await;
    assert!(texts(carol, general).is_empty());
}

/// The path of the one fork proof in the bucket.
async fn only_fork_proof_in(bucket: &Bucket) -> String {
    let proofs = bucket.fork_proofs().await.unwrap();
    let [proof] = proofs.as_slice() else {
        panic!("one proof, not {}", proofs.len());
    };
    proof.path.clone()
}

/// Ops heard live are still compared with the bucket's copy: a sweep reads
/// past what it has checked against the bucket, not past what it holds, so a
/// different version uploaded for the same seq is proved forked within one
/// sweep, even though we hold everything that segment covers.
#[tokio::test(flavor = "multi_thread")]
async fn a_version_heard_live_is_checked_against_the_bucket_within_a_sweep() {
    let fork = fork().await;
    let admin = &fork.admin;
    admin.receive(fork.for_admin.clone()).unwrap();
    admin.receive(fork.after.clone()).unwrap();
    fork.bucket
        .put_segment(&[fork.for_carol.clone(), fork.after.clone()])
        .await
        .unwrap();

    admin.sync().await;

    assert!(
        admin
            .state()
            .store
            .is_forked(&fork.bob.me(), fork.for_admin.seq)
            .unwrap()
    );
}

/// Junk over a range we hold (real headers, garbage contents) must not count
/// as checked against the bucket, or the author could plant it, wait for a
/// sweep, then upload a different version that nobody ever compares.
#[tokio::test(flavor = "multi_thread")]
async fn junk_over_a_held_range_cannot_hide_a_later_upload() {
    let fork = fork().await;
    let admin = &fork.admin;
    admin.receive(fork.for_admin.clone()).unwrap();
    admin.receive(fork.after.clone()).unwrap();
    let junk = |real: &Envelope| Envelope {
        ciphertext: vec![7; 64],
        ..real.clone()
    };
    fork.bucket
        .put_segment(&[junk(&fork.for_admin), junk(&fork.after)])
        .await
        .unwrap();
    admin.sync().await;

    fork.bucket
        .put_segment(&[fork.for_carol.clone(), fork.after.clone()])
        .await
        .unwrap();
    admin.sync().await;

    assert!(
        admin
            .state()
            .store
            .is_forked(&fork.bob.me(), fork.for_admin.seq)
            .unwrap()
    );
}

/// Junk its author signed is kept whole (it is small) though it never
/// applies, so if they also send a real op for that seq, the two are proved
/// a fork rather than the real one being quietly ignored.
#[tokio::test(flavor = "multi_thread")]
async fn junk_then_a_real_op_for_the_same_seq_is_proved_a_fork() {
    let fork = fork().await;
    let (admin, bob) = (&fork.admin, &fork.bob);
    let (epoch, key) = current_key(bob);
    let seq = fork.for_admin.seq;
    let junk = postcard::to_stdvec(&(crate::crypto::OP_FORMAT, "not an op")).unwrap();
    admin
        .receive(crate::crypto::seal_bytes(&bob.inner.identity, &key, epoch, seq, junk).unwrap())
        .unwrap();

    admin.receive(fork.for_admin.clone()).unwrap();

    assert!(admin.state().store.is_forked(&bob.me(), seq).unwrap());
}
