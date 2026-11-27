//! Linking a new device from the device the person already uses: the code,
//! the check code both screens show, the confirmation, and the admin's app
//! adding the device. The new device's side is played here with the same
//! parts its app uses.

use super::support::*;
use crate::{
    DeviceEnrollment, Networking, Workspace,
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
        device_linking::STOPS_BEFORE_FORGETTING_READING,
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
    assert!(matches!(answer(&bucket, &code).await, Answer::Check(_)));
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

    bob.cancel_link();

    assert!(bob.confirm_link(&real).await.is_err());
    assert!(bob.pending_link().await.unwrap().is_none());
    assert!(matches!(answer(&bucket, &code).await, Answer::Check(_)));
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
    assert!(!dir.join("store.db").exists(), "the enrollment is gone too");
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
    let (root, bucket, _admin, bob) = admin_and_bob().await;
    let dir = root.path().join("phone");
    let (_code, linking, reading) = reading_device(&bob, &dir, &bucket).await;
    drop(reading);
    drop(linking);
    data_dir(&dir).linking().mark_link_finishing().await.unwrap();

    let Err(error) = resume(&dir, &bucket).await else {
        panic!("a join that began can't be picked up");
    };

    assert!(format!("{error:#}").contains("Link again"), "{error:#}");
    assert!(!linking_store_of(&dir).exists());
    assert!(!dir.join("store.db").exists());
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
    assert!(!dir.join("store.db").exists());
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
    assert!(!dir.join("store.db").exists());
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
