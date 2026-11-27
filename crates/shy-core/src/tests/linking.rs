//! Linking a new device from the device the person already uses: the code,
//! the check code both screens show, the confirmation, and the admin's app
//! adding the device. The new device's side is played here with the same
//! parts its app uses.

use super::support::*;
use crate::{
    DeviceEnrollment, Networking, Workspace,
    membership::invite::JoinMailbox,
    membership::{
        device_change::DeviceChange,
        handover::{Answer, open_handover},
        keyring::KeyringEntry,
        link::{CheckReading, LinkCode, LinkRequest, Platform, read_check, seal_link_request},
        link_answers::{Outcome, Reason, SignedOutcome, SignedWelcome},
    },
    ops::Op,
    private::crypto::SealingKeyPair,
    transport::bucket::{Bucket, read_only::ReadingConnector},
    workspace::{
        CheckOnNewDevice, DeviceLink, DeviceLinking, FileSource, JoinCancel, LinkRefused, NewFile,
        ResumedLink,
        device_linking::{STOPS_BEFORE_FORGETTING_READING, STOPS_BEFORE_JOINING},
        linking_store::{LINKING_LIFETIME_MS, backdate_link},
    },
};

struct NewDevice {
    enrollment: DeviceEnrollment,
    request: LinkRequest,
    handover: SealingKeyPair,
}

/// A new device in `dir` asking to be linked through `code`.
async fn ask_through(code: &LinkCode, dir: &std::path::Path, bucket: &Bucket) -> NewDevice {
    let link = DeviceLink {
        workspace_id: code.workspace_id.clone(),
        workspace_name: code.workspace_name.clone(),
        admin: code.admin,
        admin_mls_key: code.admin_mls_key.clone(),
        person: code.person,
    };
    let (enrollment, device_request) = DeviceEnrollment::begin(data_dir(dir), link).await.unwrap();
    let (request, handover) = enrollment
        .link_request(code, &device_request, Platform::Linux)
        .unwrap();
    bucket
        .put_link_slot(&code.token, "request", seal_link_request(code, &request).unwrap())
        .await
        .unwrap();
    NewDevice {
        enrollment,
        request,
        handover,
    }
}

async fn answer(bucket: &Bucket, code: &LinkCode) -> Answer {
    Answer::decode(&bucket.link_slot(&code.token, "answer").await.unwrap().unwrap()).unwrap()
}

/// The code the new device shows, from its own keys.
async fn code_on(new: &NewDevice, bucket: &Bucket, code: &LinkCode) -> String {
    let Answer::Check(check) = answer(bucket, code).await else {
        panic!("the check comes first");
    };
    match read_check(code, &new.request.device, &new.handover.public, &check).unwrap() {
        CheckReading::Code(shown) => shown,
        CheckReading::SomeoneElse => panic!("the check answers this device"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_linked_with_a_code_reads_at_once_and_is_added_by_the_admins_app() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let code = bob.create_test_link_code().unwrap();
    let new = ask_through(&code, &root.path().join("phone"), &bucket).await;

    let shown = bob.pending_link().await.unwrap().unwrap();
    let on_new_device = code_on(&new, &bucket, &code).await;
    assert!(shown.choices.contains(&on_new_device));
    assert_eq!(shown.platform, Platform::Linux);

    bob.confirm_link(&on_new_device).await.unwrap();

    let Answer::Handover(sealed) = answer(&bucket, &code).await else {
        panic!("the hand-over replaces the check");
    };
    let handover = open_handover(&code, &new.request.device, &new.handover.secret, &sealed).unwrap();
    assert!(
        handover
            .history
            .epochs
            .iter()
            .all(|entry| entry.epoch <= handover.epoch)
    );
    bob.sync().await;
    admin.sync().await;
    let welcome_bytes = bucket
        .link_reply(&code.token, &new.request.device, "welcome")
        .await
        .unwrap()
        .unwrap();
    let welcome = SignedWelcome::decode(&welcome_bytes)
        .unwrap()
        .read(&admin.me(), &code.workspace_id, &code.token, &new.request.device)
        .unwrap();
    let phone = new
        .enrollment
        .finish_with(welcome, connector(&bucket), Networking::Offline)
        .await
        .unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    for member in [&admin, &bob, &phone] {
        assert_eq!(member.state().store.person_of(&phone.me()).unwrap(), bob.me());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_code_answers_one_request_only() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let code = bob.create_test_link_code().unwrap();
    let first = ask_through(&code, &root.path().join("phone"), &bucket).await;
    let shown = bob.pending_link().await.unwrap().unwrap();

    // Another device puts its request in the slot afterwards.
    let _second = ask_through(&code, &root.path().join("intruder"), &bucket).await;

    assert_eq!(bob.pending_link().await.unwrap().unwrap(), shown);
    assert!(shown.choices.contains(&code_on(&first, &bucket, &code).await));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_choices_stay_the_same_while_the_check_is_open() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let code = bob.create_test_link_code().unwrap();
    let new = ask_through(&code, &root.path().join("phone"), &bucket).await;

    let shown = bob.pending_link().await.unwrap().unwrap();

    for _ in 0..3 {
        assert_eq!(bob.pending_link().await.unwrap().unwrap(), shown);
    }
    let real = code_on(&new, &bucket, &code).await;
    assert_eq!(shown.choices.iter().filter(|choice| **choice == real).count(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_pick_ends_the_link_and_spends_the_code() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let code = bob.create_test_link_code().unwrap();
    let new = ask_through(&code, &root.path().join("phone"), &bucket).await;
    let shown = bob.pending_link().await.unwrap().unwrap();
    let real = code_on(&new, &bucket, &code).await;
    let decoy = shown.choices.iter().find(|choice| **choice != real).unwrap();

    let refused = format!("{:#}", bob.confirm_link(decoy).await.unwrap_err());

    assert!(refused.contains("wasn't the number"), "{refused}");
    assert!(bob.confirm_link(&real).await.is_err(), "the code is spent");
    assert!(bob.pending_link().await.unwrap().is_none());
    assert!(
        matches!(answer(&bucket, &code).await, Answer::Stopped(_)),
        "the new device is told"
    );
    assert!(bob.state().store.pending_links().unwrap().is_empty());
    assert!(bob.state().store.device_changes().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn junk_in_the_request_slot_is_ignored_and_the_real_request_still_works() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let code = bob.create_test_link_code().unwrap();
    bucket
        .put_link_slot(&code.token, "request", b"junk".to_vec())
        .await
        .unwrap();

    assert_eq!(bob.pending_link().await.unwrap(), None);
    ask_through(&code, &root.path().join("phone"), &bucket).await;
    assert!(bob.pending_link().await.unwrap().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn none_of_these_ends_the_link_and_spends_the_code() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let code = bob.create_test_link_code().unwrap();
    let new = ask_through(&code, &root.path().join("phone"), &bucket).await;
    bob.pending_link().await.unwrap().unwrap();
    let real = code_on(&new, &bucket, &code).await;

    bob.cancel_link().await.unwrap();

    assert!(bob.confirm_link(&real).await.is_err());
    assert!(bob.pending_link().await.unwrap().is_none());
    assert!(
        matches!(answer(&bucket, &code).await, Answer::Stopped(_)),
        "the new device is told"
    );
    assert!(bob.state().store.pending_links().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn linking_is_refused_before_any_code_when_the_person_has_five_devices() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    for index in 0..4 {
        link_device(
            &admin,
            &bob,
            &root.path().join(format!("device-{index}")),
            &bucket,
        )
        .await;
    }
    sync_all(&[&admin, &bob]).await;

    let refused = format!("{:#}", bob.create_test_link_code().unwrap_err());

    assert!(refused.contains("already have 5 devices"), "{refused}");
    assert!(bob.pending_link().await.unwrap().is_none());
}

/// `voucher`'s person links a new device in `dir`, which opens its
/// reading store from the hand-over.
async fn reading_device(
    voucher: &Workspace,
    dir: &std::path::Path,
    bucket: &Bucket,
) -> (LinkCode, DeviceLinking<BucketLinkMailbox>, Workspace) {
    let (code, mut linking) = handed_over(voucher, dir, bucket).await;
    let reading = linking
        .wait_for_handover_with(&JoinCancel::new(), ReadingConnector::over(connector(bucket)))
        .await
        .unwrap();
    (code, linking, reading)
}

/// `voucher`'s person confirms a new device in `dir`, which has its
/// hand-over waiting.
async fn handed_over(
    voucher: &Workspace,
    dir: &std::path::Path,
    bucket: &Bucket,
) -> (LinkCode, DeviceLinking<BucketLinkMailbox>) {
    let code = voucher.create_test_link_code().unwrap();
    let linking = begin_linking(&code, dir, bucket).await;
    voucher.pending_link().await.unwrap().unwrap();
    let Some(CheckOnNewDevice::Code(shown)) = linking.check_now().await.unwrap() else {
        panic!("the check answers this device");
    };
    voucher.confirm_link(&shown).await.unwrap();
    (code, linking)
}

fn has_message(member: &Workspace, channel: &str, text: &str) -> bool {
    member
        .messages(channel, None, 50)
        .unwrap()
        .iter()
        .any(|message| message.text == text)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_device_reads_what_was_handed_over_at_once_and_writes_nothing() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "before the link", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    let dir = root.path().join("phone");
    let (_code, _linking, reading) = reading_device(&bob, &dir, &bucket).await;
    reading.sync().await;

    assert_eq!(reading.status().error, None);
    assert!(has_message(&reading, &general, "before the link"));
    assert_ne!(reading.me(), bob.me());
    assert_eq!(reading.my_person(), bob.me());
    let refused = format!("{:#}", reading.send_message(&general, "hello", None).unwrap_err());
    assert!(refused.contains("can read now"), "{refused}");
    let file = NewFile {
        name: "note.txt".into(),
        media_type: "text/plain".into(),
        source: FileSource::Bytes(b"hi".to_vec()),
    };
    let refused = format!(
        "{:#}",
        reading
            .send_files(&general, "", None, vec![file])
            .await
            .unwrap_err()
    );
    assert!(refused.contains("can read now"), "{refused}");
    assert_eq!(reading.state().store.last_seq(&reading.me()).unwrap(), 0);
}

/// Even past the app's own checks, the bucket a reading device holds
/// writes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_reading_devices_bucket_refuses_every_write() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let (code, _linking, reading) = reading_device(&bob, &root.path().join("phone"), &bucket).await;

    let reading_bucket = reading.bucket();
    assert!(
        reading_bucket
            .put_link_slot(&code.token, "request", b"x".to_vec())
            .await
            .is_err()
    );
    assert!(
        reading_bucket
            .delete_link_slot(&code.token, "answer")
            .await
            .is_err()
    );
    assert!(bucket.link_slot(&code.token, "answer").await.unwrap().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hand_over_with_keys_past_its_epoch_opens_nothing() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let (_code, mut linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    let mut handover = linking.handover_now().await.unwrap().unwrap();
    handover.history.epochs.push(KeyringEntry {
        epoch: handover.epoch + 1,
        key: [7; 32],
        members: Vec::new(),
    });

    let opened = linking
        .open_reading_from(handover, ReadingConnector::over(connector(&bucket)))
        .await;

    assert!(opened.is_err());
}

/// What is written after the epoch handed over waits, unread and without an
/// error, until the device is added.
#[tokio::test(flavor = "multi_thread")]
async fn ops_past_the_handed_over_epoch_wait_unread() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let (_code, _linking, reading) = reading_device(&bob, &root.path().join("phone"), &bucket).await;
    admin.commit_keys_only().await.unwrap();
    admin.send_message(&general, "after the hand-over", None).unwrap();
    admin.sync().await;

    reading.sync().await;

    assert_eq!(reading.status().error, None);
    assert!(!has_message(&reading, &general, "after the hand-over"));
}

/// The new device's side, as its app drives it.
async fn begin_linking(
    code: &LinkCode,
    dir: &std::path::Path,
    bucket: &Bucket,
) -> DeviceLinking<BucketLinkMailbox> {
    let mailbox = BucketLinkMailbox {
        bucket: bucket.clone(),
        token: code.token,
    };
    DeviceLinking::begin_with(data_dir(dir), code.clone(), Platform::Linux, mailbox)
        .await
        .unwrap()
}

/// The new device's enrollment, its store, MLS database and key, is gone.
fn assert_enrollment_gone(dir: &std::path::Path) {
    assert!(
        !dir.join(crate::workspace::STORE_FILE).exists(),
        "the enrollment's store is gone"
    );
    assert!(
        !dir.join(crate::workspace::MLS_FILE).exists(),
        "its MLS database is gone"
    );
    assert!(!dir.with_extension("key").exists(), "its key is gone");
}

fn linking_store_of(dir: &std::path::Path) -> std::path::PathBuf {
    let mut path = dir.as_os_str().to_owned();
    path.push(".linking");
    path.into()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_device_links_from_its_own_side_and_keeps_what_it_typed_while_reading() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "before the link", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let dir = root.path().join("phone");
    let code = bob.create_test_link_code().unwrap();
    let mut linking = begin_linking(&code, &dir, &bucket).await;
    let cancel = JoinCancel::new();

    let choices = bob.pending_link().await.unwrap().unwrap().choices;
    let CheckOnNewDevice::Code(shown) = linking.wait_for_check(&cancel).await.unwrap() else {
        panic!("the check answers this device");
    };
    assert!(choices.contains(&shown));
    bob.confirm_link(&shown).await.unwrap();
    let reading = linking
        .wait_for_handover_with(&cancel, ReadingConnector::over(connector(&bucket)))
        .await
        .unwrap();
    reading.sync().await;
    let before = reading
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .find(|message| message.text == "before the link")
        .unwrap();
    reading.mark_read(&general, &before.id).unwrap();
    reading.set_draft(&general, "typed while reading").unwrap();
    bob.sync().await;
    admin.sync().await;

    let phone = linking
        .wait_until_added_with(reading, &cancel, connector(&bucket), Networking::Offline)
        .await
        .unwrap();

    sync_all(&[&admin, &bob, &phone]).await;
    assert!(
        phone
            .drafts()
            .unwrap()
            .iter()
            .any(|draft| draft.text == "typed while reading")
    );
    let channel = phone
        .channels()
        .unwrap()
        .into_iter()
        .find(|channel| channel.id == general)
        .unwrap();
    let read = channel.last_read.unwrap();
    assert_eq!((read.lamport, read.device), (before.lamport, before.device));
    assert_eq!(phone.state().store.person_of(&phone.me()).unwrap(), bob.me());
    phone.send_message(&general, "sent once added", None).unwrap();
    assert!(!linking_store_of(&dir).exists(), "the reading store is gone");
    let me = phone.me();
    drop(phone);
    let reopened = Workspace::open_with(&data_dir(&dir), connector(&bucket), Networking::Offline)
        .await
        .unwrap();
    assert_eq!(
        reopened.me(),
        me,
        "deleting the reading store kept this device's key"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_signed_refusal_wipes_both_stores_on_the_new_device() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let code = bob.create_test_link_code().unwrap();
    let mut linking = begin_linking(&code, &dir, &bucket).await;
    let cancel = JoinCancel::new();
    bob.pending_link().await.unwrap().unwrap();
    let CheckOnNewDevice::Code(shown) = linking.wait_for_check(&cancel).await.unwrap() else {
        panic!("the check answers this device");
    };
    bob.confirm_link(&shown).await.unwrap();
    let reading = linking
        .wait_for_handover_with(&cancel, ReadingConnector::over(connector(&bucket)))
        .await
        .unwrap();
    // The person stops the link from their other device before it is added.
    bob.write(Op::DeviceChange {
        change: DeviceChange::Remove {
            devices: vec![reading.me()],
        },
    })
    .unwrap();
    bob.sync().await;
    admin.sync().await;

    let Err(refused) = linking
        .wait_until_added_with(reading, &cancel, connector(&bucket), Networking::Offline)
        .await
    else {
        panic!("the link was cancelled");
    };

    assert!(matches!(
        refused.downcast_ref::<LinkRefused>(),
        Some(LinkRefused(Reason::Cancelled))
    ));
    assert!(!linking_store_of(&dir).exists());
    assert!(!Workspace::exists(&dir));
    assert_enrollment_gone(&dir);
}

/// Someone with the bucket keys can't make the new device give up.
#[tokio::test(flavor = "multi_thread")]
async fn an_outcome_the_admin_did_not_sign_wipes_nothing() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let code = bob.create_test_link_code().unwrap();
    let mut linking = begin_linking(&code, &dir, &bucket).await;
    let cancel = JoinCancel::new();
    bob.pending_link().await.unwrap().unwrap();
    let CheckOnNewDevice::Code(shown) = linking.wait_for_check(&cancel).await.unwrap() else {
        panic!("the check answers this device");
    };
    bob.confirm_link(&shown).await.unwrap();
    let reading = linking
        .wait_for_handover_with(&cancel, ReadingConnector::over(connector(&bucket)))
        .await
        .unwrap();
    let forged = SignedOutcome::sign(
        &bob.inner.identity,
        &code.workspace_id,
        Outcome {
            token: code.token,
            device: reading.me(),
            certificate_hash: [0; 32],
            reason: Reason::Cancelled,
        },
    )
    .unwrap()
    .encode()
    .unwrap();
    bucket
        .put_link_reply(&code.token, &reading.me(), "outcome", forged)
        .await
        .unwrap();
    bucket
        .put_link_reply(&code.token, &reading.me(), "welcome", b"junk".to_vec())
        .await
        .unwrap();

    assert!(linking.added_now(&reading).await.unwrap().is_none());
    assert!(linking_store_of(&dir).exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_device_whose_code_someone_else_used_is_told_so() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let code = bob.create_test_link_code().unwrap();
    let _first = ask_through(&code, &root.path().join("intruder"), &bucket).await;
    bob.pending_link().await.unwrap().unwrap();
    let mailbox = BucketLinkMailbox {
        bucket: bucket.clone(),
        token: code.token,
    };
    // The intruder's request was opened first; this device's comes after.
    let linking = DeviceLinking::begin_with(
        data_dir(&root.path().join("phone")),
        code.clone(),
        Platform::Linux,
        mailbox,
    )
    .await
    .unwrap();

    assert_eq!(
        linking.check_now().await.unwrap(),
        Some(CheckOnNewDevice::SomeoneElse)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn junk_in_the_answer_slot_does_not_end_the_link() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let code = bob.create_test_link_code().unwrap();
    let linking = begin_linking(&code, &root.path().join("phone"), &bucket).await;
    bucket
        .put_link_slot(&code.token, "answer", b"junk".to_vec())
        .await
        .unwrap();

    assert_eq!(linking.check_now().await.unwrap(), None);
    bob.pending_link().await.unwrap().unwrap();
    assert!(matches!(
        linking.check_now().await.unwrap(),
        Some(CheckOnNewDevice::Code(_))
    ));
}

async fn resume(
    dir: &std::path::Path,
    bucket: &Bucket,
) -> anyhow::Result<Option<ResumedLink<BucketLinkMailbox>>> {
    let mailbox_bucket = bucket.clone();
    DeviceLinking::resume_with(
        data_dir(dir),
        move |code| BucketLinkMailbox {
            bucket: mailbox_bucket,
            token: code.token,
        },
        ReadingConnector::over(connector(bucket)),
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_device_restarted_before_the_hand_over_picks_its_link_up() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let (_code, linking) = handed_over(&bob, &dir, &bucket).await;
    drop(linking);

    let Some(ResumedLink::Waiting(mut linking)) = resume(&dir, &bucket).await.unwrap() else {
        panic!("the link waits for its hand-over");
    };

    assert_eq!(
        linking.check_now().await.unwrap(),
        Some(CheckOnNewDevice::Confirmed)
    );
    let cancel = JoinCancel::new();
    let reading = linking
        .wait_for_handover_with(&cancel, ReadingConnector::over(connector(&bucket)))
        .await
        .unwrap();
    bob.sync().await;
    admin.sync().await;
    let phone = linking
        .wait_until_added_with(reading, &cancel, connector(&bucket), Networking::Offline)
        .await
        .unwrap();
    assert_eq!(phone.state().store.person_of(&phone.me()).unwrap(), bob.me());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_device_restarted_while_reading_reads_again_and_finishes() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "before the link", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let dir = root.path().join("phone");
    let (_code, linking, reading) = reading_device(&bob, &dir, &bucket).await;
    reading.sync().await;
    reading.set_draft(&general, "typed before the restart").unwrap();
    drop(reading);
    drop(linking);

    let Some(ResumedLink::Reading(linking, reading)) = resume(&dir, &bucket).await.unwrap() else {
        panic!("the link reads again");
    };
    reading.sync().await;

    assert!(has_message(&reading, &general, "before the link"));
    bob.sync().await;
    admin.sync().await;
    let phone = linking
        .wait_until_added_with(
            reading,
            &JoinCancel::new(),
            connector(&bucket),
            Networking::Offline,
        )
        .await
        .unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    assert!(
        phone
            .drafts()
            .unwrap()
            .iter()
            .any(|draft| draft.text == "typed before the restart")
    );
}

/// MLS spends the device's key as it reads the welcome, so a join that
/// stopped partway can't run again: both stores go, and it says so (R4).
#[tokio::test(flavor = "multi_thread")]
async fn a_join_that_stopped_partway_wipes_both_stores_and_says_link_again() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let (_code, linking, reading) = reading_device(&bob, &dir, &bucket).await;
    bob.sync().await;
    admin.sync().await;
    STOPS_BEFORE_JOINING
        .lock()
        .unwrap()
        .push(data_dir(&dir).linking().path);
    let stopped = linking
        .wait_until_added_with(
            reading,
            &JoinCancel::new(),
            connector(&bucket),
            Networking::Offline,
        )
        .await;
    assert!(stopped.is_err(), "stopped as the app would, just before joining");

    let Err(error) = resume(&dir, &bucket).await else {
        panic!("a join that began can't be picked up");
    };

    assert!(format!("{error:#}").contains("Link again"), "{error:#}");
    assert!(!linking_store_of(&dir).exists());
    assert_enrollment_gone(&dir);
    assert!(resume(&dir, &bucket).await.unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_link_not_added_within_a_week_is_wiped_on_restart() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let (_code, linking, reading) = reading_device(&bob, &dir, &bucket).await;
    drop(reading);
    drop(linking);
    backdate_link(&data_dir(&dir).linking(), LINKING_LIFETIME_MS)
        .await
        .unwrap();

    let Err(error) = resume(&dir, &bucket).await else {
        panic!("a week-old link isn't picked up");
    };

    assert!(format!("{error:#}").contains("within a week"), "{error:#}");
    assert!(!linking_store_of(&dir).exists());
    assert_enrollment_gone(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_with_no_link_resumes_nothing() {
    let root = tempfile::tempdir().unwrap();

    assert!(
        resume(&root.path().join("phone"), &memory_bucket())
            .await
            .unwrap()
            .is_none()
    );
}

/// The one-time secret goes once the hand-over opens: a hand-over put up
/// again later opens nothing.
#[tokio::test(flavor = "multi_thread")]
async fn the_one_time_secret_is_gone_once_the_hand_over_opens() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let (_code, linking, reading) = reading_device(&bob, &dir, &bucket).await;

    assert!(linking.handover_now().await.is_err());
    drop(reading);
    drop(linking);
    let Some(ResumedLink::Reading(linking, _reading)) = resume(&dir, &bucket).await.unwrap() else {
        panic!("the link reads again");
    };
    assert!(linking.handover_now().await.is_err());
}

/// The app stopped after the join, before the reading store was deleted:
/// the next start deletes it and its key, and the workspace opens with its
/// own key and the drafts it carried.
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_after_the_join_deletes_only_the_reading_store() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob]).await;
    let dir = root.path().join("phone");
    let (_code, linking, reading) = reading_device(&bob, &dir, &bucket).await;
    reading.sync().await;
    reading.set_draft(&general, "typed while reading").unwrap();
    bob.sync().await;
    admin.sync().await;
    let linking_dir = data_dir(&dir).linking();
    STOPS_BEFORE_FORGETTING_READING
        .lock()
        .unwrap()
        .push(linking_dir.path.clone());
    let phone = linking
        .wait_until_added_with(
            reading,
            &JoinCancel::new(),
            connector(&bucket),
            Networking::Offline,
        )
        .await
        .unwrap();
    let me = phone.me();
    drop(phone);
    assert!(linking_store_of(&dir).exists(), "the stop left the reading store");
    let linking_key = dir.with_extension("key.linking");
    assert!(linking_key.exists());

    assert!(resume(&dir, &bucket).await.unwrap().is_none());

    assert!(!linking_store_of(&dir).exists());
    assert!(!linking_key.exists(), "the reading store's key is gone");
    assert!(dir.with_extension("key").exists(), "the real key is untouched");
    let reopened = Workspace::open_with(&data_dir(&dir), connector(&bucket), Networking::Offline)
        .await
        .unwrap();
    assert_eq!(reopened.me(), me);
    reopened.sync().await;
    assert!(
        reopened
            .drafts()
            .unwrap()
            .iter()
            .any(|draft| draft.text == "typed while reading")
    );
}

fn edit_pending_link(voucher: &Workspace, change: impl FnOnce(&mut crate::storage::PendingLink)) {
    let store = &voucher.state().store;
    let mut link = store.pending_links().unwrap().remove(0);
    change(&mut link);
    store.put_pending_link(&link).unwrap();
}

fn links_asked_by(member: &Workspace, voucher: &Workspace) -> usize {
    member
        .state()
        .store
        .device_changes()
        .unwrap()
        .iter()
        .filter(|change| change.author == voucher.me() && matches!(change.change, DeviceChange::Link { .. }))
        .count()
}

/// A restart between saving the confirmed link and putting the hand-over
/// up leaves the hand-over to the next pass.
#[tokio::test(flavor = "multi_thread")]
async fn a_hand_over_left_undone_is_put_up_by_the_next_pass() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let (code, linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    let handover = bucket.link_slot(&code.token, "answer").await.unwrap().unwrap();
    bucket
        .put_link_slot(&code.token, "answer", b"lost".to_vec())
        .await
        .unwrap();
    edit_pending_link(&bob, |link| link.is_handed_over = false);

    bob.sync().await;

    assert_ne!(
        bucket.link_slot(&code.token, "answer").await.unwrap().unwrap(),
        b"lost".as_slice()
    );
    assert!(linking.handover_now().await.unwrap().is_some());
    assert!(bob.state().store.pending_links().unwrap()[0].is_handed_over);
    assert_eq!(links_asked_by(&bob, &bob), 1, "the link is asked for once");
    drop(handover);
}

/// The admin's app saw the certificate only after it went stale: the
/// voucher vouches again for the same device, and it is added.
#[tokio::test(flavor = "multi_thread")]
async fn a_stale_certificate_is_vouched_for_again_and_the_device_added() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let (_code, linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    for _ in 0..5 {
        admin.commit_keys_only().await.unwrap();
    }
    bob.sync().await;
    admin.sync().await;
    let device = bob.state().store.pending_links().unwrap()[0].request.device;
    assert!(
        !admin.state().mls.is_member(&device).unwrap(),
        "refused as stale first"
    );

    bob.sync().await;
    // The new link goes out with the next push.
    bob.sync().await;
    admin.sync().await;

    assert!(admin.state().mls.is_member(&device).unwrap());
    assert_eq!(links_asked_by(&admin, &bob), 2);
    drop(linking);
}

/// Not after the person removed it: a stale link for a device they took
/// back is never asked for again.
#[tokio::test(flavor = "multi_thread")]
async fn a_device_the_person_removed_is_not_vouched_for_again() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let (_code, _linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    let device = bob.state().store.pending_links().unwrap()[0].request.device;
    for _ in 0..5 {
        admin.commit_keys_only().await.unwrap();
    }
    bob.sync().await;
    admin.sync().await;
    bob.write(Op::DeviceChange {
        change: DeviceChange::Remove {
            devices: vec![device],
        },
    })
    .unwrap();

    bob.sync().await;
    admin.sync().await;

    assert!(!admin.state().mls.is_member(&device).unwrap());
    assert_eq!(links_asked_by(&admin, &bob), 1);
    assert!(bob.state().store.pending_links().unwrap()[0].is_refused);
}

/// Once the device is added and the answer link has run out, the link's
/// slots go and the record ends.
#[tokio::test(flavor = "multi_thread")]
async fn once_the_device_is_added_its_link_slots_go_and_the_record_ends() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let (code, linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    bob.sync().await;
    admin.sync().await;

    bob.sync().await;

    // The new device may not have fetched its hand-over yet: it stays while
    // the answer link works.
    assert!(linking.handover_now().await.unwrap().is_some());
    edit_pending_link(&bob, |link| link.answer_until_ms = 0);
    bob.sync().await;
    assert!(bob.state().store.pending_links().unwrap().is_empty());
    assert!(bucket.link_slot(&code.token, "request").await.unwrap().is_none());
    assert!(bucket.link_slot(&code.token, "answer").await.unwrap().is_none());
}

/// A hand-over that never went up while the new device could read it:
/// the voucher cancels the link, so no device that can't join is added.
#[tokio::test(flavor = "multi_thread")]
async fn a_link_whose_hand_over_never_went_up_in_time_is_cancelled() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let (code, _linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    let device = bob.state().store.pending_links().unwrap()[0].request.device;
    edit_pending_link(&bob, |link| {
        link.is_handed_over = false;
        link.answer_until_ms = 0;
    });

    bob.sync().await;
    // The cancellation goes out with the next push, before the admin's app
    // has seen the link.
    bob.sync().await;
    admin.sync().await;

    assert!(!admin.state().mls.is_member(&device).unwrap());
    let outcome = bucket
        .link_reply(&code.token, &device, "outcome")
        .await
        .unwrap()
        .unwrap();
    let signed = SignedOutcome::decode(&outcome).unwrap();
    let read = signed
        .read(&admin.me(), &code.workspace_id, &code.token, &device)
        .unwrap();
    assert_eq!(read.reason, Reason::Cancelled);
}

/// A refused link's outcome stays for the new device to read until the
/// voucher's record ends; then it and the slots go.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_links_outcome_stays_until_the_record_ends() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let (code, _linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    let device = bob.state().store.pending_links().unwrap()[0].request.device;
    bob.write(Op::DeviceChange {
        change: DeviceChange::Remove {
            devices: vec![device],
        },
    })
    .unwrap();
    bob.sync().await;
    admin.sync().await;

    bob.sync().await;

    assert!(bob.state().store.pending_links().unwrap()[0].is_refused);
    assert!(
        bucket
            .link_reply(&code.token, &device, "outcome")
            .await
            .unwrap()
            .is_some()
    );
    edit_pending_link(&bob, |link| link.until_ms = 0);
    bob.sync().await;
    assert!(bob.state().store.pending_links().unwrap().is_empty());
    assert!(
        bucket
            .link_reply(&code.token, &device, "outcome")
            .await
            .unwrap()
            .is_none()
    );
    assert!(bucket.link_slot(&code.token, "answer").await.unwrap().is_none());
}

/// Once the answer link has run out, nobody can read the slots: they go,
/// once, while the record waits for the admin's app.
#[tokio::test(flavor = "multi_thread")]
async fn the_link_slots_go_once_the_answer_link_runs_out() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let (code, _linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    edit_pending_link(&bob, |link| link.answer_until_ms = 0);

    bob.sync().await;

    assert!(bucket.link_slot(&code.token, "request").await.unwrap().is_none());
    assert!(bucket.link_slot(&code.token, "answer").await.unwrap().is_none());
    let link = &bob.state().store.pending_links().unwrap()[0];
    assert!(link.are_slots_deleted && !link.is_refused);
}

/// The admin's app added the device before the cancellation arrived: it
/// never got its hand-over, so it can never join, and it is removed.
#[tokio::test(flavor = "multi_thread")]
async fn a_device_added_without_its_hand_over_is_removed() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let (_code, _linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    let device = bob.state().store.pending_links().unwrap()[0].request.device;
    bob.sync().await;
    admin.sync().await;
    assert!(admin.state().mls.is_member(&device).unwrap());
    edit_pending_link(&bob, |link| {
        link.is_handed_over = false;
        link.answer_until_ms = 0;
    });

    bob.sync().await;
    bob.sync().await;
    admin.sync().await;

    assert!(!admin.state().mls.is_member(&device).unwrap());
    bob.sync().await;
    assert!(bob.state().store.pending_links().unwrap().is_empty());
}

/// The device that linked another knows what it runs on.
#[tokio::test(flavor = "multi_thread")]
async fn the_voucher_shows_the_platform_of_the_device_it_linked() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let (_code, _linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    bob.sync().await;
    admin.sync().await;
    bob.sync().await;

    let devices = bob.my_devices().unwrap();

    assert_eq!(devices[1].platform, Some(Platform::Linux));
    assert_eq!(admin.my_devices().unwrap()[0].platform, None);
}

/// A link one of your devices asked for shows on your other devices, and
/// to nobody else.
#[tokio::test(flavor = "multi_thread")]
async fn a_link_from_one_of_your_devices_shows_a_notice_on_the_others_only() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let (_code, _linking) = handed_over(&bob, &root.path().join("tablet"), &bucket).await;
    let tablet = bob.state().store.pending_links().unwrap()[0].request.device;

    sync_all(&[&bob, &phone, &admin]).await;

    let notices = phone.device_notices().unwrap();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].device, crate::views::member_id(&tablet));
    assert_eq!(notices[0].kind, crate::views::DeviceNoticeKind::Linked);
    assert!(!notices[0].is_seen);
    assert!(
        bob.device_notices().unwrap().is_empty(),
        "not on the device that linked it"
    );
    assert!(admin.device_notices().unwrap().is_empty());

    phone.mark_device_notices_seen().unwrap();
    phone.state().store.rebuild_views(phone.apply_context()).unwrap();
    let notices = phone.device_notices().unwrap();
    assert_eq!(notices.len(), 1, "a rebuild notes it once");
    assert!(notices[0].is_seen, "and keeps it seen");
}

/// A device still waiting a week after it began wipes what it holds, even
/// while its app stays open.
#[tokio::test(flavor = "multi_thread")]
async fn a_link_not_added_within_a_week_is_wiped_while_waiting() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let (_code, mut linking, reading) = reading_device(&bob, &dir, &bucket).await;
    linking.backdate(LINKING_LIFETIME_MS);

    let Err(error) = linking
        .wait_until_added_with(
            reading,
            &JoinCancel::new(),
            connector(&bucket),
            Networking::Offline,
        )
        .await
    else {
        panic!("a week-old link isn't waited on");
    };

    assert!(format!("{error:#}").contains("within a week"), "{error:#}");
    assert!(!linking_store_of(&dir).exists());
    assert_enrollment_gone(&dir);
}

/// "Not you? Remove it": a stolen phone links a stranger's tablet; the
/// person's first device sees it as linking, removes it, and the link is
/// refused, never added.
#[tokio::test(flavor = "multi_thread")]
async fn a_device_still_linking_can_be_removed_from_another_of_your_devices() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let (code, _linking) = handed_over(&phone, &root.path().join("tablet"), &bucket).await;
    let tablet = phone.state().store.pending_links().unwrap()[0].request.device;
    phone.sync().await;
    bob.sync().await;

    assert_eq!(
        bob.device_notices().unwrap()[0].device,
        crate::views::member_id(&tablet)
    );
    let linking = bob
        .my_devices()
        .unwrap()
        .into_iter()
        .find(|device| device.id == crate::views::member_id(&tablet))
        .unwrap();
    assert!(linking.is_linking);
    bob.remove_my_device(&linking.id).await.unwrap();
    bob.sync().await;
    admin.sync().await;

    assert!(!admin.state().mls.is_member(&tablet).unwrap());
    let outcome = bucket
        .link_reply(&code.token, &tablet, "outcome")
        .await
        .unwrap()
        .unwrap();
    let signed = SignedOutcome::decode(&outcome).unwrap();
    let read = signed
        .read(&admin.me(), &code.workspace_id, &code.token, &tablet)
        .unwrap();
    assert_eq!(read.reason, Reason::Cancelled);
}

/// A wrong pick tells the new device the link stopped: it wipes what it
/// holds and says so, rather than waiting an hour.
#[tokio::test(flavor = "multi_thread")]
async fn the_new_device_is_told_when_the_person_picks_another_number() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let code = bob.create_test_link_code().unwrap();
    let mut linking = begin_linking(&code, &dir, &bucket).await;
    let choices = bob.pending_link().await.unwrap().unwrap().choices;
    let Some(CheckOnNewDevice::Code(shown)) = linking.check_now().await.unwrap() else {
        panic!("the check answers this device");
    };
    let decoy = choices.iter().find(|choice| **choice != shown).unwrap();
    assert!(bob.confirm_link(decoy).await.is_err());

    let Err(error) = linking.wait_for_check(&JoinCancel::new()).await else {
        panic!("the link stopped");
    };

    assert!(matches!(
        error.downcast_ref::<LinkRefused>(),
        Some(LinkRefused(Reason::Cancelled))
    ));
    assert!(!linking_store_of(&dir).exists());
    assert_enrollment_gone(&dir);
}

/// A stop the voucher didn't sign for this device is junk.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_for_another_device_is_ignored() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let code = bob.create_test_link_code().unwrap();
    let linking = begin_linking(&code, &root.path().join("phone"), &bucket).await;
    let stranger = iroh::SecretKey::generate().public();
    let stop = crate::membership::handover::SignedStop::sign(&bob.inner.identity, &code, &stranger).unwrap();
    bucket
        .put_link_slot(&code.token, "answer", Answer::Stopped(stop).encode().unwrap())
        .await
        .unwrap();

    assert_eq!(linking.check_now().await.unwrap(), None);
}

// Rules the review asked to pin down.

/// Two links the admin's app added count as members, not also as links
/// waiting: a third code is still allowed.
#[tokio::test(flavor = "multi_thread")]
async fn links_already_added_do_not_count_twice_before_a_new_code() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    for name in ["phone", "tablet"] {
        let (_code, _linking) = handed_over(&bob, &root.path().join(name), &bucket).await;
        bob.sync().await;
        admin.sync().await;
        bob.sync().await;
    }
    assert_eq!(
        bob.state().store.pending_links().unwrap().len(),
        2,
        "kept while the answer link works"
    );
    let other = link_device(&admin, &bob, &root.path().join("watch"), &bucket).await;
    sync_all(&[&admin, &bob, &other]).await;

    assert!(
        bob.create_test_link_code().is_ok(),
        "four devices and nothing waiting"
    );
}

/// A refused link is a try for the day, not a device.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_link_counts_as_a_try_but_not_as_a_device() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    for index in 0..3 {
        link_device(
            &admin,
            &bob,
            &root.path().join(format!("linked-{index}")),
            &bucket,
        )
        .await;
    }
    sync_all(&[&admin, &bob]).await;
    let (_code, _linking) = handed_over(&bob, &root.path().join("refused"), &bucket).await;
    let device = bob.state().store.pending_links().unwrap()[0].request.device;
    bob.write(Op::DeviceChange {
        change: DeviceChange::Remove {
            devices: vec![device],
        },
    })
    .unwrap();
    bob.sync().await;
    admin.sync().await;
    bob.sync().await;
    assert!(bob.state().store.pending_links().unwrap()[0].is_refused);

    assert!(
        bob.create_test_link_code().is_ok(),
        "four devices, the refused link not one"
    );
}

/// A link confirmed but not yet added counts as a device.
#[tokio::test(flavor = "multi_thread")]
async fn a_link_waiting_for_the_admins_app_counts_as_a_device() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    for index in 0..3 {
        link_device(
            &admin,
            &bob,
            &root.path().join(format!("linked-{index}")),
            &bucket,
        )
        .await;
    }
    sync_all(&[&admin, &bob]).await;
    let (_code, _linking) = handed_over(&bob, &root.path().join("waiting"), &bucket).await;

    let refused = format!("{:#}", bob.create_test_link_code().unwrap_err());

    assert!(refused.contains("already have 5 devices"), "{refused}");
}

/// Five links today, even of devices removed since, use up the day.
#[tokio::test(flavor = "multi_thread")]
async fn five_links_today_refuse_a_sixth_code() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    for index in 0..5 {
        let device = link_device(
            &admin,
            &bob,
            &root.path().join(format!("linked-{index}")),
            &bucket,
        )
        .await;
        sync_all(&[&admin, &bob, &device]).await;
        admin
            .remove_devices(&[crate::views::member_id(&device.me())])
            .await
            .unwrap();
        bob.sync().await;
    }

    let refused = format!("{:#}", bob.create_test_link_code().unwrap_err());

    assert!(refused.contains("linked several devices today"), "{refused}");
}

/// A hand-over put up after the epoch moved holds no key past the
/// certificate's epoch.
#[tokio::test(flavor = "multi_thread")]
async fn a_hand_over_holds_no_key_past_its_certificates_epoch() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let (code, linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    let certificate_epoch = bob.state().store.pending_links().unwrap()[0].certificate.epoch;
    bucket.delete_link_slot(&code.token, "answer").await.unwrap();
    edit_pending_link(&bob, |link| link.is_handed_over = false);
    admin.commit_keys_only().await.unwrap();

    bob.sync().await;

    assert!(epoch_of(&bob) > certificate_epoch);
    let handover = linking.handover_now().await.unwrap().unwrap();
    assert_eq!(handover.epoch, certificate_epoch);
    assert!(
        handover
            .history
            .epochs
            .iter()
            .all(|entry| entry.epoch <= certificate_epoch)
    );
    assert!(
        handover
            .history
            .epochs
            .iter()
            .any(|entry| entry.epoch == certificate_epoch)
    );
}

/// A stale outcome makes the new device wait, never wipe.
#[tokio::test(flavor = "multi_thread")]
async fn a_stale_outcome_wipes_nothing_on_the_new_device() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let (code, linking, reading) = reading_device(&bob, &dir, &bucket).await;
    let stale = SignedOutcome::sign(
        &admin.inner.identity,
        &code.workspace_id,
        Outcome {
            token: code.token,
            device: reading.me(),
            certificate_hash: [0; 32],
            reason: Reason::Stale,
        },
    )
    .unwrap()
    .encode()
    .unwrap();
    bucket
        .put_link_reply(&code.token, &reading.me(), "outcome", stale)
        .await
        .unwrap();

    assert!(linking.added_now(&reading).await.unwrap().is_none());
    assert!(linking_store_of(&dir).exists());
}

/// An outcome the admin signed for an earlier certificate doesn't end the
/// voucher's current link.
#[tokio::test(flavor = "multi_thread")]
async fn an_outcome_for_an_earlier_certificate_is_passed_over() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let (code, _linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    let device = bob.state().store.pending_links().unwrap()[0].request.device;
    let earlier = SignedOutcome::sign(
        &admin.inner.identity,
        &code.workspace_id,
        Outcome {
            token: code.token,
            device,
            certificate_hash: [0; 32],
            reason: Reason::Cancelled,
        },
    )
    .unwrap()
    .encode()
    .unwrap();
    bucket
        .put_link_reply(&code.token, &device, "outcome", earlier)
        .await
        .unwrap();

    bob.sync().await;

    assert!(!bob.state().store.pending_links().unwrap()[0].is_refused);
}

/// A cancel written by a device removed since counts for nothing: the
/// stale link is vouched for again and added.
#[tokio::test(flavor = "multi_thread")]
async fn a_removed_devices_cancel_does_not_stop_a_new_vouch() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let (_code, _linking) = handed_over(&bob, &root.path().join("tablet"), &bucket).await;
    let tablet = bob.state().store.pending_links().unwrap()[0].request.device;
    phone
        .write(Op::DeviceChange {
            change: DeviceChange::Remove {
                devices: vec![tablet],
            },
        })
        .unwrap();
    phone.sync().await;
    admin
        .remove_devices(&[crate::views::member_id(&phone.me())])
        .await
        .unwrap();
    for _ in 0..5 {
        admin.commit_keys_only().await.unwrap();
    }
    bob.sync().await;
    admin.sync().await;
    assert!(!admin.state().mls.is_member(&tablet).unwrap(), "stale first");

    bob.sync().await;
    bob.sync().await;
    admin.sync().await;

    assert!(admin.state().mls.is_member(&tablet).unwrap());
}

/// A finished link's reading store goes once the last handle the app kept
/// to it closes.
#[tokio::test(flavor = "multi_thread")]
async fn finishing_with_another_handle_open_lets_the_store_go_once_it_closes() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let (_code, linking, reading) = reading_device(&bob, &dir, &bucket).await;
    let kept_by_the_app = reading.clone();
    bob.sync().await;
    admin.sync().await;

    let phone = linking
        .wait_until_added_with(
            reading,
            &JoinCancel::new(),
            connector(&bucket),
            Networking::Offline,
        )
        .await
        .unwrap();

    assert_eq!(phone.state().store.person_of(&phone.me()).unwrap(), bob.me());
    drop(kept_by_the_app);
    for _ in 0..50 {
        if !linking_store_of(&dir).exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(!linking_store_of(&dir).exists());
    assert!(!dir.with_extension("key.linking").exists());
}

/// The guarantee belongs to the folder: a linking store doesn't open as a
/// member's workspace.
#[tokio::test(flavor = "multi_thread")]
async fn a_linking_store_does_not_open_as_a_members_workspace() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let (_code, linking, reading) = reading_device(&bob, &dir, &bucket).await;
    drop(reading);
    drop(linking);

    let opened =
        Workspace::open_with(&data_dir(&dir).linking(), connector(&bucket), Networking::Offline).await;

    let refused = format!("{:#}", opened.err().unwrap());
    assert!(refused.contains("opens only for reading"), "{refused}");
}

/// Past the answer link, a confirmation asks for nothing.
#[tokio::test(flavor = "multi_thread")]
async fn confirming_after_the_answer_link_ran_out_asks_for_nothing() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let code = bob.create_test_link_code().unwrap();
    let linking = begin_linking(&code, &root.path().join("phone"), &bucket).await;
    bob.pending_link().await.unwrap().unwrap();
    let Some(CheckOnNewDevice::Code(shown)) = linking.check_now().await.unwrap() else {
        panic!("the check answers this device");
    };
    bob.age_open_link(crate::membership::link::ANSWER_LIFETIME_MS);

    let refused = format!("{:#}", bob.confirm_link(&shown).await.unwrap_err());

    assert!(refused.contains("This code ran out"), "{refused}");
    assert_eq!(links_asked_by(&bob, &bob), 0);
    assert!(bob.state().store.pending_links().unwrap().is_empty());
}

/// A check whose upload failed goes up the next time it is asked for.
#[tokio::test(flavor = "multi_thread")]
async fn a_check_that_failed_to_go_up_is_put_up_again() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let code = bob.create_test_link_code().unwrap();
    let linking = begin_linking(&code, &root.path().join("phone"), &bucket).await;
    let writable = bob.bucket();
    *bob.inner.bucket.write().unwrap() =
        crate::transport::bucket::read_only::ReadingBucket::over(writable.clone()).into_bucket();
    assert!(bob.pending_link().await.is_err());
    *bob.inner.bucket.write().unwrap() = writable;

    let shown = bob.pending_link().await.unwrap().unwrap();

    let Some(CheckOnNewDevice::Code(code_on_device)) = linking.check_now().await.unwrap() else {
        panic!("the check went up");
    };
    assert!(shown.choices.contains(&code_on_device));
}

/// A hand-over someone took away while the new device can still read it
/// goes up again.
#[tokio::test(flavor = "multi_thread")]
async fn a_hand_over_taken_away_goes_up_again() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let (code, linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    bucket.delete_link_slot(&code.token, "answer").await.unwrap();

    bob.sync().await;

    assert!(linking.handover_now().await.unwrap().is_some());
}

/// Once the device is added and the answer link ran out, its outcome goes
/// with the slots.
#[tokio::test(flavor = "multi_thread")]
async fn an_added_devices_outcome_goes_with_its_slots() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let (code, _linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    let device = bob.state().store.pending_links().unwrap()[0].request.device;
    bob.sync().await;
    admin.sync().await;
    bucket
        .put_link_reply(&code.token, &device, "outcome", b"left behind".to_vec())
        .await
        .unwrap();
    edit_pending_link(&bob, |link| link.answer_until_ms = 0);

    bob.sync().await;

    assert!(
        bucket
            .link_reply(&code.token, &device, "outcome")
            .await
            .unwrap()
            .is_none()
    );
}

/// A stop after the reading store was complete, before its secret was
/// deleted, picks the link up as reading, and the secret goes.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_before_the_secret_was_deleted_resumes_as_reading() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let (_code, linking, reading) = reading_device(&bob, &dir, &bucket).await;
    reading
        .state()
        .store
        .set_meta(crate::workspace::linking_store::HANDOVER_SECRET_META, &[1; 32])
        .unwrap();
    drop(reading);
    drop(linking);

    let Some(ResumedLink::Reading(linking, reading)) = resume(&dir, &bucket).await.unwrap() else {
        panic!("the link reads again");
    };

    assert!(
        reading
            .state()
            .store
            .meta(crate::workspace::linking_store::HANDOVER_SECRET_META)
            .unwrap()
            .is_none()
    );
    assert!(linking.handover_now().await.is_err(), "nothing to open it with");
}

/// Stopping on the new device wipes both stores at once.
#[tokio::test(flavor = "multi_thread")]
async fn cancelling_on_the_new_device_wipes_both_stores() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let (_code, linking, reading) = reading_device(&bob, &dir, &bucket).await;

    linking.cancel(Some(reading)).await.unwrap();

    assert!(!linking_store_of(&dir).exists());
    assert_enrollment_gone(&dir);
    assert!(resume(&dir, &bucket).await.unwrap().is_none());
}

/// The probe: a store still held open by the app is never deleted under a
/// newer link. A new link waits until the last handle closes, then works.
#[tokio::test(flavor = "multi_thread")]
async fn a_new_link_waits_for_the_last_ones_store_to_close() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let (_code, linking, reading) = reading_device(&bob, &dir, &bucket).await;
    let kept_by_the_app = reading.clone();
    linking.cancel(Some(reading)).await.unwrap();
    let code = bob.create_test_link_code().unwrap();
    let mailbox = BucketLinkMailbox {
        bucket: bucket.clone(),
        token: code.token,
    };

    let refused = DeviceLinking::begin_with(data_dir(&dir), code.clone(), Platform::Linux, mailbox).await;

    let refused = format!("{:#}", refused.err().unwrap());
    assert!(refused.contains("Finishing the last link"), "{refused}");
    drop(kept_by_the_app);
    for _ in 0..50 {
        if !linking_store_of(&dir).exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let mut linking = begin_linking(&code, &dir, &bucket).await;
    bob.pending_link().await.unwrap().unwrap();
    let Some(CheckOnNewDevice::Code(shown)) = linking.check_now().await.unwrap() else {
        panic!("the check answers the new link");
    };
    bob.confirm_link(&shown).await.unwrap();
    let reading = linking
        .wait_for_handover_with(&JoinCancel::new(), ReadingConnector::over(connector(&bucket)))
        .await
        .unwrap();
    bob.sync().await;
    admin.sync().await;
    let phone = linking
        .wait_until_added_with(
            reading,
            &JoinCancel::new(),
            connector(&bucket),
            Networking::Offline,
        )
        .await
        .unwrap();
    assert_eq!(phone.state().store.person_of(&phone.me()).unwrap(), bob.me());
}

/// The delete armed for one link leaves a newer link's store alone.
#[tokio::test(flavor = "multi_thread")]
async fn the_delete_armed_for_one_link_leaves_a_newer_one_alone() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let first = bob.create_test_link_code().unwrap();
    begin_linking(&first, &dir, &bucket)
        .await
        .cancel(None)
        .await
        .unwrap();
    let second = bob.create_test_link_code().unwrap();
    let linking = begin_linking(&second, &dir, &bucket).await;

    data_dir(&dir)
        .linking()
        .forget_reading_of(&first.token)
        .await
        .unwrap();

    assert!(linking_store_of(&dir).exists());
    drop(linking);
    assert!(matches!(
        resume(&dir, &bucket).await.unwrap(),
        Some(ResumedLink::Waiting(_))
    ));
}

/// A mailbox whose first fetches fail, as when the storage can't be reached.
struct FlakyMailbox {
    inner: BucketLinkMailbox,
    failures_left: std::sync::atomic::AtomicUsize,
}

impl JoinMailbox for FlakyMailbox {
    async fn post_request(&self, request: Vec<u8>) -> anyhow::Result<()> {
        self.inner.post_request(request).await
    }

    async fn fetch_answer(&self) -> anyhow::Result<Option<Vec<u8>>> {
        let failures_left = &self.failures_left;
        if failures_left.load(std::sync::atomic::Ordering::SeqCst) > 0 {
            failures_left.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            anyhow::bail!("can't reach the workspace's storage");
        }
        self.inner.fetch_answer().await
    }
}

/// A fetch that fails while waiting for the check is retried, not a reason
/// to wipe the link.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_fetch_while_waiting_for_the_check_is_retried() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let code = bob.create_test_link_code().unwrap();
    let mailbox = FlakyMailbox {
        inner: BucketLinkMailbox {
            bucket: bucket.clone(),
            token: code.token,
        },
        failures_left: std::sync::atomic::AtomicUsize::new(1),
    };
    let mut linking = DeviceLinking::begin_with(data_dir(&dir), code, Platform::Linux, mailbox)
        .await
        .unwrap();
    bob.pending_link().await.unwrap().unwrap();

    let check = linking.wait_for_check(&JoinCancel::new()).await.unwrap();

    assert!(matches!(check, CheckOnNewDevice::Code(_)));
    assert!(linking_store_of(&dir).exists());
}

/// A link the admin's app refused stops showing as linking on the person's
/// other devices: its voucher names it in a removal everyone sees.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_link_stops_showing_as_linking_on_your_other_devices() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    for index in 0..2 {
        link_device(
            &admin,
            &bob,
            &root.path().join(format!("linked-{index}")),
            &bucket,
        )
        .await;
    }
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let (_code, _linking) = handed_over(&bob, &root.path().join("tablet"), &bucket).await;
    let tablet = crate::views::member_id(&bob.state().store.pending_links().unwrap()[0].request.device);
    bob.sync().await;
    phone.sync().await;
    assert!(
        phone
            .my_devices()
            .unwrap()
            .iter()
            .any(|device| device.id == tablet && device.is_linking)
    );
    // A fifth device added meanwhile: the admin's app refuses the tablet.
    link_device(&admin, &bob, &root.path().join("watch"), &bucket).await;
    admin.sync().await;

    bob.sync().await;
    bob.sync().await;
    phone.sync().await;

    assert!(bob.state().store.pending_links().unwrap()[0].is_refused);
    assert!(
        !phone
            .my_devices()
            .unwrap()
            .iter()
            .any(|device| device.id == tablet)
    );
    assert!(!bob.my_devices().unwrap().iter().any(|device| device.id == tablet));
}

/// A wipe that left the store held open, then a stop before the last
/// handle closed: the app's other handle was told to let go, and the next
/// start deletes the store rather than picking the link up.
#[tokio::test(flavor = "multi_thread")]
async fn a_wiped_link_left_open_is_told_and_deleted_at_the_next_start() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let (_code, linking, reading) = reading_device(&bob, &dir, &bucket).await;
    let kept_by_the_app = reading.clone();
    let mut told = kept_by_the_app.subscribe();
    linking.cancel(Some(reading)).await.unwrap();

    let mut was_told = false;
    while let Ok(change) = told.try_recv() {
        was_told |= change == crate::views::Change::Removed;
    }
    assert!(was_told, "the app's handle is told to let go");
    kept_by_the_app.inner.forget_on_close.disarm();
    drop(kept_by_the_app);
    assert!(linking_store_of(&dir).exists(), "as if the app stopped first");

    assert!(resume(&dir, &bucket).await.unwrap().is_none());
    assert!(!linking_store_of(&dir).exists());
}

/// Someone else's devices see nothing of a person's links in progress.
#[tokio::test(flavor = "multi_thread")]
async fn another_person_sees_none_of_your_links_in_progress() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let (_code, _linking) = handed_over(&bob, &root.path().join("phone"), &bucket).await;
    bob.sync().await;

    carol.sync().await;

    assert_eq!(links_asked_by(&carol, &bob), 1, "Carol read the link op");
    assert!(bob.my_devices().unwrap().iter().any(|device| device.is_linking));
    assert!(!carol.my_devices().unwrap().iter().any(|device| device.is_linking));
}

/// Refused tries alone use up the day.
#[tokio::test(flavor = "multi_thread")]
async fn five_refused_tries_today_refuse_a_sixth_code() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    for index in 0..5 {
        let (_code, _linking) = handed_over(&bob, &root.path().join(format!("try-{index}")), &bucket).await;
        let device = bob.state().store.pending_links().unwrap()[index].request.device;
        bob.write(Op::DeviceChange {
            change: DeviceChange::Remove {
                devices: vec![device],
            },
        })
        .unwrap();
        bob.sync().await;
        admin.sync().await;
        bob.sync().await;
    }
    assert!(
        bob.state()
            .store
            .pending_links()
            .unwrap()
            .iter()
            .all(|link| link.is_refused)
    );

    let refused = format!("{:#}", bob.create_test_link_code().unwrap_err());

    assert!(refused.contains("linked several devices today"), "{refused}");
}

/// A folder holding a link in progress isn't created over as a workspace.
#[tokio::test(flavor = "multi_thread")]
async fn a_linking_store_is_not_created_over() {
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let code = bob.create_test_link_code().unwrap();
    drop(begin_linking(&code, &dir, &bucket).await);

    let created = Workspace::create_with(
        &data_dir(&dir).linking(),
        "another".into(),
        "Other",
        crate::Profile {
            name: "Mallory",
            is_bot: false,
        },
        test_bucket_config(),
        connector(&bucket),
        Networking::Offline,
    )
    .await;

    let refused = format!("{:#}", created.err().unwrap());
    assert!(refused.contains("opens only for reading"), "{refused}");
}
