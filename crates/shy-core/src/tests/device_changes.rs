//! Device changes as every member judges them: who may ask to link or
//! remove which device, and that the result is the same everywhere.

use iroh::SecretKey;

use super::support::*;
use crate::{
    Workspace,
    membership::{device::DeviceCertificate, device_change::DeviceChange, link::LinkedDevice},
    ops::{Envelope, Op},
    storage::RecordedChange,
};

/// One forger per author for a whole test: each seals the author's next op.
fn change(forger: &mut Forger, change: DeviceChange) -> Envelope {
    forger.seal(Op::DeviceChange { change })
}

fn deliver(envelope: &Envelope, members: &[&Workspace]) {
    for member in members {
        // A void change is skipped, which may surface as an error.
        let _ = member.receive(envelope.clone());
    }
}

/// The changes `member` kept, without the time each was shown (a device's own).
fn kept(member: &Workspace) -> Vec<(iroh::PublicKey, u64, iroh::PublicKey, DeviceChange)> {
    member
        .state()
        .store
        .device_changes()
        .unwrap()
        .into_iter()
        .map(
            |RecordedChange {
                 author,
                 seq,
                 person,
                 change,
                 ..
             }| (author, seq, person, change),
        )
        .collect()
}

fn link_of(certificate: DeviceCertificate) -> DeviceChange {
    DeviceChange::Link {
        token: [1; 16],
        linked: LinkedDevice {
            key_package: vec![2; 200],
            binding: vec![3; 64],
            acceptance: certificate.device_signature.clone(),
        },
        certificate: Box::new(certificate),
    }
}

fn removal(devices: &[iroh::PublicKey]) -> DeviceChange {
    DeviceChange::Remove {
        devices: devices.to_vec(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_persons_device_can_ask_to_link_a_device_it_vouched_for() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let mut bobs = Forger::new(&bob);
    let new_device = SecretKey::generate();
    let link = link_of(device_certificate(
        &bob.inner.identity,
        &bob.me(),
        &new_device,
        epoch_of(&bob),
    ));

    deliver(&change(&mut bobs, link.clone()), &[&admin, &carol]);

    for member in [&admin, &carol] {
        let kept = kept(member);
        assert_eq!(kept.len(), 1);
        assert_eq!((kept[0].0, kept[0].2), (bob.me(), bob.me()));
        assert_eq!(kept[0].3, link);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_link_not_vouched_for_by_the_persons_own_device_is_void() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let mut bobs = Forger::new(&bob);
    let epoch = epoch_of(&bob);
    let mut forged = device_certificate(&bob.inner.identity, &bob.me(), &SecretKey::generate(), epoch);
    forged.voucher_signature[0] ^= 1;

    for certificate in [
        // For another person.
        device_certificate(&bob.inner.identity, &carol.me(), &SecretKey::generate(), epoch),
        // Signed by another device.
        device_certificate(&admin.inner.identity, &bob.me(), &SecretKey::generate(), epoch),
        forged,
    ] {
        deliver(&change(&mut bobs, link_of(certificate)), &[&admin, &carol]);
    }

    assert!(kept(&admin).is_empty());
    assert!(kept(&carol).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removal_may_name_only_the_persons_own_devices() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &carol, &phone]).await;
    let mut bobs = Forger::new(&bob);
    let mut phones = Forger::new(&phone);

    deliver(&change(&mut bobs, removal(&[carol.me()])), &[&admin]);
    deliver(&change(&mut bobs, removal(&[phone.me(), carol.me()])), &[&admin]);
    assert!(kept(&admin).is_empty());

    deliver(&change(&mut bobs, removal(&[phone.me()])), &[&admin]);
    deliver(&change(&mut phones, removal(&[phone.me()])), &[&admin]);
    let kept = kept(&admin);
    assert_eq!(
        kept.len(),
        2,
        "the first device removing its phone, and the phone itself"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_linked_device_cannot_ask_to_remove_the_first_device() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let mut phones = Forger::new(&phone);

    deliver(&change(&mut phones, removal(&[bob.me()])), &[&admin, &bob]);

    assert!(kept(&admin).is_empty());
    assert!(kept(&bob).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_admins_first_device_cannot_be_removed_by_request() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let admins_phone = link_device(&admin, &admin, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &admins_phone]).await;
    let mut admins = Forger::new(&admin);

    deliver(&change(&mut admins, removal(&[admin.me()])), &[&bob]);
    deliver(
        &change(&mut admins, removal(&[admin.me(), admins_phone.me()])),
        &[&bob],
    );

    assert!(kept(&bob).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn device_changes_are_the_same_in_any_order_and_applied_twice() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let dave = join(&admin, &root.path().join("dave"), &bucket, "Dave", false).await;
    sync_all(&[&admin, &bob, &carol, &phone, &dave]).await;
    let mut bobs = Forger::new(&bob);
    let mut phones = Forger::new(&phone);
    let mut carols = Forger::new(&carol);
    let new_device = SecretKey::generate();
    let changes = [
        change(
            &mut bobs,
            link_of(device_certificate(
                &bob.inner.identity,
                &bob.me(),
                &new_device,
                epoch_of(&bob),
            )),
        ),
        change(&mut phones, removal(&[phone.me()])),
        change(&mut carols, removal(&[carol.me()])),
    ];

    for change in &changes {
        deliver(change, &[&admin]);
    }
    for change in changes.iter().rev().chain(&changes) {
        deliver(change, &[&dave]);
    }

    assert_eq!(kept(&admin).len(), 3);
    assert_eq!(kept(&dave), kept(&admin));
    dave.state().store.rebuild_views(dave.apply_context()).unwrap();
    assert_eq!(kept(&dave), kept(&admin), "the same after a rebuild");
}

// ---- The admin's app acting on device changes ----

/// A new device in `dir` that `voucher` asked the admin to link, through
/// `voucher`'s forger, with the change delivered to the admin.
struct AskedLink {
    enrollment: crate::DeviceEnrollment,
    device: iroh::PublicKey,
    token: crate::membership::link::Token,
}

async fn ask_to_link(
    voucher: &Workspace,
    forger: &mut Forger<'_>,
    dir: &std::path::Path,
    admin: &Workspace,
) -> AskedLink {
    ask_to_link_at(voucher, forger, dir, admin, epoch_of(voucher)).await
}

async fn ask_to_link_at(
    voucher: &Workspace,
    forger: &mut Forger<'_>,
    dir: &std::path::Path,
    admin: &Workspace,
    epoch: u64,
) -> AskedLink {
    let (enrollment, request) = begin_link(voucher, dir).await;
    let certificate = DeviceCertificate::vouch(
        &voucher.inner.identity,
        &voucher.inner.config.workspace_id,
        voucher.my_person(),
        request.device,
        epoch,
        request.acceptance.clone(),
    )
    .unwrap();
    let token = rand::random();
    let link = DeviceChange::Link {
        token,
        linked: LinkedDevice {
            key_package: request.key_package.clone(),
            binding: request.binding.clone(),
            acceptance: request.acceptance.clone(),
        },
        certificate: Box::new(certificate),
    };
    admin.receive(change(forger, link)).unwrap();
    AskedLink {
        enrollment,
        device: request.device,
        token,
    }
}

/// The admin's reply to `asked` in slot `name`.
async fn reply(bucket: &crate::transport::bucket::Bucket, asked: &AskedLink, name: &str) -> Option<Vec<u8>> {
    bucket
        .link_reply(&asked.token, &asked.device, name)
        .await
        .unwrap()
        .map(|bytes| bytes.to_vec())
}

fn outcome_in(
    bytes: &[u8],
    admin: &Workspace,
    asked: &AskedLink,
) -> crate::membership::link_answers::Outcome {
    let signed = crate::membership::link_answers::SignedOutcome::decode(bytes).unwrap();
    signed
        .read(
            &admin.me(),
            &admin.inner.config.workspace_id,
            &asked.token,
            &asked.device,
        )
        .unwrap()
        .clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_link_the_person_asked_for_is_added_once_and_its_welcome_put_up() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let mut bobs = Forger::new(&bob);
    let asked = ask_to_link(&bob, &mut bobs, &root.path().join("phone"), &admin).await;

    admin.sync().await;

    let welcome = crate::membership::link_answers::SignedWelcome::decode(
        &reply(&bucket, &asked, "welcome").await.unwrap(),
    )
    .unwrap()
    .read(
        &admin.me(),
        &admin.inner.config.workspace_id,
        &asked.token,
        &asked.device,
    )
    .unwrap();
    let phone = asked
        .enrollment
        .finish_with(welcome, connector(&bucket), crate::Networking::Offline)
        .await
        .unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    for member in [&admin, &bob, &phone] {
        assert_eq!(member.state().store.person_of(&phone.me()).unwrap(), bob.me());
    }
    let epoch = epoch_of(&admin);
    admin.sync().await;
    assert_eq!(epoch_of(&admin), epoch, "acted on once");
    admin.state().store.rebuild_views(admin.apply_context()).unwrap();
    admin.sync().await;
    assert_eq!(epoch_of(&admin), epoch, "a rebuild doesn't make it act again");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_link_cancelled_before_it_was_added_is_refused_with_one_key_only_commit() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let mut bobs = Forger::new(&bob);
    let asked = ask_to_link(&bob, &mut bobs, &root.path().join("phone"), &admin).await;
    admin
        .receive(change(&mut bobs, removal(&[asked.device])))
        .unwrap();
    let epoch = epoch_of(&admin);

    admin.sync().await;

    let outcome = outcome_in(&reply(&bucket, &asked, "outcome").await.unwrap(), &admin, &asked);
    assert_eq!(outcome.reason, crate::membership::link_answers::Reason::Cancelled);
    assert_eq!(epoch_of(&admin), epoch + 1, "one key-only commit");
    assert!(!admin.state().mls.is_member(&asked.device).unwrap());
    assert!(reply(&bucket, &asked, "welcome").await.is_none());
    bob.sync().await;
    assert_eq!(epoch_of(&bob), epoch + 1, "members follow it");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_certificate_is_a_retry_without_a_key_only_commit() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    for name in ["Carol", "Dave", "Erin", "Fay", "Gus"] {
        join(&admin, &root.path().join(name), &bucket, name, false).await;
    }
    sync_all(&[&admin, &bob]).await;
    let mut bobs = Forger::new(&bob);
    let stale_epoch = epoch_of(&bob) - 5;
    let asked = ask_to_link_at(&bob, &mut bobs, &root.path().join("phone"), &admin, stale_epoch).await;
    let epoch = epoch_of(&admin);

    admin.sync().await;

    let outcome = outcome_in(&reply(&bucket, &asked, "outcome").await.unwrap(), &admin, &asked);
    assert_eq!(outcome.reason, crate::membership::link_answers::Reason::Stale);
    assert_eq!(epoch_of(&admin), epoch, "no key-only commit for a retry");
}

#[tokio::test(flavor = "multi_thread")]
async fn two_devices_removing_each_other_lose_only_the_second_requester() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let tablet = link_device(&admin, &bob, &root.path().join("tablet"), &bucket).await;
    sync_all(&[&admin, &bob, &phone, &tablet]).await;
    let (mut phones, mut tablets) = (Forger::new(&phone), Forger::new(&tablet));
    admin
        .receive(change(&mut phones, removal(&[tablet.me()])))
        .unwrap();
    admin
        .receive(change(&mut tablets, removal(&[phone.me()])))
        .unwrap();

    admin.sync().await;

    let state = admin.state();
    assert!(state.mls.is_member(&phone.me()).unwrap());
    assert!(!state.mls.is_member(&tablet.me()).unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_thiefs_removal_cannot_outrun_the_first_devices_in_one_pass() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let tablet = link_device(&admin, &bob, &root.path().join("tablet"), &bucket).await;
    sync_all(&[&admin, &bob, &phone, &tablet]).await;
    let (mut phones, mut bobs) = (Forger::new(&phone), Forger::new(&bob));
    // The stolen phone asks first; Bob's laptop asks to remove the phone.
    admin
        .receive(change(&mut phones, removal(&[tablet.me()])))
        .unwrap();
    admin.receive(change(&mut bobs, removal(&[phone.me()]))).unwrap();

    admin.sync().await;

    let state = admin.state();
    assert!(!state.mls.is_member(&phone.me()).unwrap());
    assert!(state.mls.is_member(&tablet.me()).unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_from_a_device_removed_before_the_admin_acts_is_void() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let mut phones = Forger::new(&phone);
    let asked = ask_to_link(&phone, &mut phones, &root.path().join("tablet"), &admin).await;
    admin.remove_devices(&[phone.my_device_id()]).await.unwrap();
    let epoch = epoch_of(&admin);

    admin.sync().await;

    assert!(!admin.state().mls.is_member(&asked.device).unwrap());
    assert!(reply(&bucket, &asked, "welcome").await.is_none());
    // Skipped, not refused: no outcome, and no key-only commit.
    assert!(reply(&bucket, &asked, "outcome").await.is_none());
    assert_eq!(epoch_of(&admin), epoch);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_persons_last_device_is_not_removed_by_request() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let mut bobs = Forger::new(&bob);
    admin.receive(change(&mut bobs, removal(&[bob.me()]))).unwrap();

    admin.sync().await;

    assert!(admin.state().mls.is_member(&bob.me()).unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn owed_work_left_by_a_crash_after_the_acted_mark_is_finished_on_the_next_pass() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let mut bobs = Forger::new(&bob);
    let added = ask_to_link(&bob, &mut bobs, &root.path().join("phone"), &admin).await;
    admin.sync().await;
    // As if the app stopped after the add's commit, before the grant was
    // sealed and the welcome put up.
    bucket
        .delete_link_reply(&added.token, &added.device, "welcome")
        .await
        .unwrap();
    let unsealed = crate::storage::OwedWelcome {
        sealed_grant: None,
        ..admin.state().store.owed_welcomes().unwrap().remove(0)
    };
    admin.state().store.put_owed_welcome(&unsealed).unwrap();
    admin.inner.welcomes_sent.lock().unwrap().clear();
    // And as if it stopped after refusing a link, before the key-only commit
    // and the outcome's upload.
    let refused = ask_to_link(&bob, &mut bobs, &root.path().join("tablet"), &admin).await;
    let outcome = crate::membership::link_answers::SignedOutcome::sign(
        &admin.inner.identity,
        &admin.inner.config.workspace_id,
        crate::membership::link_answers::Outcome {
            token: refused.token,
            device: refused.device,
            certificate_hash: [0; 32],
            reason: crate::membership::link_answers::Reason::Refused,
        },
    )
    .unwrap()
    .encode()
    .unwrap();
    {
        let state = admin.state();
        let change = state.store.unacted_device_changes().unwrap().remove(0);
        state
            .store
            .atomically(|store| {
                store.mark_acted(&change, crate::storage::Acted::Refused, 0)?;
                store.put_owed_outcome(&refused.token, &refused.device, &outcome)?;
                store.owe_key_only_commit()
            })
            .unwrap();
    }
    let epoch = epoch_of(&admin);

    admin.sync().await;

    assert!(
        reply(&bucket, &added, "welcome").await.is_some(),
        "the welcome went up after all"
    );
    assert!(reply(&bucket, &refused, "outcome").await.is_some());
    assert_eq!(epoch_of(&admin), epoch + 1, "the owed key-only commit was made");
    assert!(!admin.state().store.is_key_only_commit_owed().unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn another_persons_removal_cannot_cancel_a_pending_link() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let (mut bobs, mut carols) = (Forger::new(&bob), Forger::new(&carol));
    let asked = ask_to_link(&bob, &mut bobs, &root.path().join("phone"), &admin).await;
    admin
        .receive(change(&mut carols, removal(&[asked.device])))
        .unwrap();

    admin.sync().await;

    assert!(admin.state().mls.is_member(&asked.device).unwrap());
    assert_eq!(admin.state().store.person_of(&asked.device).unwrap(), bob.me());
}

/// The new key becomes public with Bob's link op; Carol names it while it
/// isn't a member, holds the op, and delivers it once the device is Bob's.
#[tokio::test(flavor = "multi_thread")]
async fn another_persons_removal_cannot_remove_a_device_once_it_is_linked() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let (mut bobs, mut carols) = (Forger::new(&bob), Forger::new(&carol));
    let asked = ask_to_link(&bob, &mut bobs, &root.path().join("phone"), &admin).await;
    let held = change(&mut carols, removal(&[asked.device]));
    admin.sync().await;
    assert!(admin.state().mls.is_member(&asked.device).unwrap());

    admin.receive(held).unwrap();
    admin.sync().await;

    assert!(
        admin.state().mls.is_member(&asked.device).unwrap(),
        "still Bob's device"
    );
}

/// The phone's removal reaches the admin before the laptop's link: it still
/// cancels the link, in a later pass.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancellation_heard_before_its_link_still_cancels_it() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let (mut bobs, mut phones) = (Forger::new(&bob), Forger::new(&phone));
    // Bob's laptop asks, but the admin hasn't heard it yet.
    let tablet_dir = root.path().join("tablet");
    let (enrollment, request) = begin_link(&bob, &tablet_dir).await;
    let certificate = vouch(&bob, &request);
    let link = change(
        &mut bobs,
        DeviceChange::Link {
            token: [5; 16],
            linked: LinkedDevice {
                key_package: request.key_package.clone(),
                binding: request.binding.clone(),
                acceptance: request.acceptance.clone(),
            },
            certificate: Box::new(certificate),
        },
    );
    admin
        .receive(change(&mut phones, removal(&[request.device])))
        .unwrap();
    admin.sync().await;

    admin.receive(link).unwrap();
    admin.sync().await;

    assert!(
        !admin.state().mls.is_member(&request.device).unwrap(),
        "the link was cancelled"
    );
    drop(enrollment);
}

/// Bob's laptop links a tablet and removes the stolen phone, which names the
/// tablet's key: the thief, removed in the same pass, cancels nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_thief_removed_in_the_same_pass_cancels_nothing() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let (mut bobs, mut phones) = (Forger::new(&bob), Forger::new(&phone));
    let asked = ask_to_link(&bob, &mut bobs, &root.path().join("tablet"), &admin).await;
    admin.receive(change(&mut bobs, removal(&[phone.me()]))).unwrap();
    admin
        .receive(change(&mut phones, removal(&[asked.device])))
        .unwrap();

    admin.sync().await;

    let state = admin.state();
    assert!(!state.mls.is_member(&phone.me()).unwrap());
    assert!(
        state.mls.is_member(&asked.device).unwrap(),
        "the tablet was added"
    );
}

/// Five refused links use up the day; the sixth is refused without another
/// key-only commit, and still gets its outcome.
#[tokio::test(flavor = "multi_thread")]
async fn refusals_count_toward_the_daily_limit_and_past_it_cost_no_epoch() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let mut bobs = Forger::new(&bob);
    for index in 0..5 {
        let asked = ask_to_link(
            &bob,
            &mut bobs,
            &root.path().join(format!("device-{index}")),
            &admin,
        )
        .await;
        admin
            .receive(change(&mut bobs, removal(&[asked.device])))
            .unwrap();
    }
    admin.sync().await;
    let epoch = epoch_of(&admin);

    let sixth = ask_to_link_at(
        &bob,
        &mut bobs,
        &root.path().join("sixth"),
        &admin,
        epoch_of(&bob),
    )
    .await;
    admin.sync().await;

    let outcome = outcome_in(&reply(&bucket, &sixth, "outcome").await.unwrap(), &admin, &sixth);
    assert_eq!(
        outcome.reason,
        crate::membership::link_answers::Reason::DailyLimit
    );
    assert_eq!(epoch_of(&admin), epoch, "no key-only commit past the limit");
}

/// A link paired with its own cancellation, again and again: past the
/// daily limit it is still cancelled, with its outcome, but costs no epoch.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_link_past_the_daily_limit_costs_no_epoch() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let mut bobs = Forger::new(&bob);
    let mut epochs = Vec::new();
    for index in 0..7 {
        let asked = ask_to_link(
            &bob,
            &mut bobs,
            &root.path().join(format!("device-{index}")),
            &admin,
        )
        .await;
        admin
            .receive(change(&mut bobs, removal(&[asked.device])))
            .unwrap();
        let epoch = epoch_of(&admin);
        admin.sync().await;
        bob.sync().await;
        let outcome = outcome_in(&reply(&bucket, &asked, "outcome").await.unwrap(), &admin, &asked);
        assert_eq!(outcome.reason, crate::membership::link_answers::Reason::Cancelled);
        epochs.push(epoch_of(&admin) - epoch);
    }

    assert_eq!(epochs, [1, 1, 1, 1, 1, 0, 0]);
}

/// A person with five devices asking for more: once the day's tries are
/// used up, each refusal still says why, but costs no epoch.
#[tokio::test(flavor = "multi_thread")]
async fn too_many_devices_past_the_daily_limit_costs_no_epoch() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    for index in 0..4 {
        link_device(
            &admin,
            &bob,
            &root.path().join(format!("linked-{index}")),
            &bucket,
        )
        .await;
    }
    admin.sync().await;
    bob.sync().await;
    let mut bobs = Forger::new(&bob);
    let mut epochs = Vec::new();
    for index in 0..2 {
        let asked = ask_to_link(
            &bob,
            &mut bobs,
            &root.path().join(format!("extra-{index}")),
            &admin,
        )
        .await;
        let epoch = epoch_of(&admin);
        admin.sync().await;
        bob.sync().await;
        let outcome = outcome_in(&reply(&bucket, &asked, "outcome").await.unwrap(), &admin, &asked);
        assert_eq!(
            outcome.reason,
            crate::membership::link_answers::Reason::TooManyDevices
        );
        epochs.push(epoch_of(&admin) - epoch);
    }

    assert_eq!(epochs, [1, 0], "four links and one refusal use up the day");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_admins_app_acts_on_nothing_while_it_catches_up_after_a_restore() {
    let (root, _bucket, admin, bob) = admin_and_bob().await;
    let mut bobs = Forger::new(&bob);
    let asked = ask_to_link(&bob, &mut bobs, &root.path().join("phone"), &admin).await;
    admin
        .state()
        .store
        .set_meta(crate::storage::CATCHING_UP_META, &[1])
        .unwrap();

    admin.act_on_device_changes().await.unwrap();

    assert!(!admin.state().mls.is_member(&asked.device).unwrap());
    assert_eq!(admin.state().store.unacted_device_changes().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removal_from_a_device_removed_first_is_skipped() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let tablet = link_device(&admin, &bob, &root.path().join("tablet"), &bucket).await;
    sync_all(&[&admin, &bob, &phone, &tablet]).await;
    // Heard before the phone's removal, acted on only after it.
    admin
        .receive(change(&mut Forger::new(&phone), removal(&[tablet.me()])))
        .unwrap();
    admin.remove_devices(&[phone.my_device_id()]).await.unwrap();

    admin.sync().await;

    assert!(admin.state().mls.is_member(&tablet.me()).unwrap());
}

/// A restore that lost the acted mark finds a device that was added (and
/// since removed): it neither refuses it nor makes a key-only commit.
#[tokio::test(flavor = "multi_thread")]
async fn a_link_for_a_device_that_was_ever_a_member_is_skipped() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let mut bobs = Forger::new(&bob);
    let asked = ask_to_link(&bob, &mut bobs, &root.path().join("phone"), &admin).await;
    admin.sync().await;
    admin
        .remove_devices(&[crate::views::member_id(&asked.device)])
        .await
        .unwrap();
    {
        let state = admin.state();
        let link = state.store.device_changes().unwrap().remove(0);
        state.store.forget_acted(&link).unwrap();
    }
    let epoch = epoch_of(&admin);

    admin.sync().await;

    assert!(reply(&bucket, &asked, "outcome").await.is_none());
    assert_eq!(epoch_of(&admin), epoch);
}

/// A pass with nothing to remove leaves the snapshot alone: a skipped link
/// changes nothing it holds.
#[tokio::test(flavor = "multi_thread")]
async fn a_pass_with_no_removal_and_only_a_skipped_link_leaves_the_snapshot_alone() {
    let (root, _bucket, admin, bob) = admin_and_bob().await;
    let mut bobs = Forger::new(&bob);
    let asked = ask_to_link(&bob, &mut bobs, &root.path().join("phone"), &admin).await;
    admin.sync().await;
    admin.sync().await;
    {
        let state = admin.state();
        let link = state.store.device_changes().unwrap().remove(0);
        state.store.forget_acted(&link).unwrap();
    }
    let generation = || admin.state().store.meta("recovery_change_generation").unwrap();
    let before = generation();

    admin.sync().await;

    assert!(admin.state().mls.is_member(&asked.device).unwrap());
    assert!(admin.state().store.unacted_device_changes().unwrap().is_empty());
    assert_eq!(generation(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_owed_key_only_commit_is_made_on_its_own() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    admin.state().store.owe_key_only_commit().unwrap();
    let epoch = epoch_of(&admin);

    admin.sync().await;

    assert_eq!(epoch_of(&admin), epoch + 1);
    assert!(!admin.state().store.is_key_only_commit_owed().unwrap());
    bob.sync().await;
    assert_eq!(epoch_of(&bob), epoch + 1);
}

/// Unconditional, as decided: even when the same pass made another commit.
#[tokio::test(flavor = "multi_thread")]
async fn a_pass_that_refused_a_link_ends_with_a_key_only_commit_after_its_other_commits() {
    let (root, _bucket, admin, bob) = admin_and_bob().await;
    let mut bobs = Forger::new(&bob);
    let cancelled = ask_to_link(&bob, &mut bobs, &root.path().join("cancelled"), &admin).await;
    admin
        .receive(change(&mut bobs, removal(&[cancelled.device])))
        .unwrap();
    let added = ask_to_link(&bob, &mut bobs, &root.path().join("added"), &admin).await;
    let epoch = epoch_of(&admin);

    admin.sync().await;

    assert!(admin.state().mls.is_member(&added.device).unwrap());
    assert_eq!(epoch_of(&admin), epoch + 2, "the add, then the key-only commit");
}

/// A stale certificate's refusal makes no commit, so only the refusal's
/// own transaction can have marked the snapshot.
#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_marks_the_recovery_snapshot_stale() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    for name in ["Carol", "Dave", "Erin", "Fay", "Gus"] {
        join(&admin, &root.path().join(name), &bucket, name, false).await;
    }
    sync_all(&[&admin, &bob]).await;
    // Whatever else the joins left the admin to do, done first.
    admin.sync().await;
    admin.sync().await;
    let generation = || admin.state().store.meta("recovery_change_generation").unwrap();
    let settled = generation();
    admin.sync().await;
    assert_eq!(generation(), settled, "a pass with nothing to do marks nothing");
    let mut bobs = Forger::new(&bob);
    let stale_epoch = epoch_of(&bob) - 5;
    ask_to_link_at(&bob, &mut bobs, &root.path().join("phone"), &admin, stale_epoch).await;
    let before = generation();

    admin.sync().await;

    assert_ne!(generation(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_only_commit_changes_the_admins_own_leaf_in_a_workspace_with_devices() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let _phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let before = admin.state().mls.own_leaf_encryption_key().unwrap();

    admin.commit_keys_only().await.unwrap();

    assert_ne!(admin.state().mls.own_leaf_encryption_key().unwrap(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_welcome_stops_being_put_up_after_seven_days_or_once_its_device_is_removed() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let mut bobs = Forger::new(&bob);
    let expired = ask_to_link(&bob, &mut bobs, &root.path().join("expired"), &admin).await;
    admin.sync().await;
    let removed = ask_to_link(&bob, &mut bobs, &root.path().join("removed"), &admin).await;
    admin.sync().await;
    assert!(reply(&bucket, &expired, "welcome").await.is_some());
    let past = crate::storage::OwedWelcome {
        until_ms: 0,
        ..admin
            .state()
            .store
            .owed_welcomes()
            .unwrap()
            .into_iter()
            .find(|owed| owed.device == expired.device)
            .unwrap()
    };
    admin.state().store.put_owed_welcome(&past).unwrap();
    admin
        .remove_devices(&[crate::views::member_id(&removed.device)])
        .await
        .unwrap();

    admin.sync().await;

    assert!(admin.state().store.owed_welcomes().unwrap().is_empty());
    for asked in [&expired, &removed] {
        assert!(reply(&bucket, asked, "welcome").await.is_none());
    }
}
