//! Your own devices: the list, and asking to remove some of them.

use super::support::*;
use crate::{Workspace, views::member_id};

fn ids(member: &Workspace) -> Vec<String> {
    member
        .my_devices()
        .unwrap()
        .into_iter()
        .map(|device| device.id)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn your_devices_list_this_one_first_and_show_what_is_known_of_the_others() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;

    let on_phone = phone.my_devices().unwrap();

    assert_eq!(ids(&phone), [member_id(&phone.me()), member_id(&bob.me())]);
    assert!(on_phone[0].is_this_device && !on_phone[0].is_first_device);
    let first = &on_phone[1];
    assert!(first.is_first_device && first.linked_ms.is_none());
    assert!(first.linked_devices.is_empty(), "never this device");
    let on_bob = bob.my_devices().unwrap();
    let linked = &on_bob[1];
    assert_eq!(linked.id, member_id(&phone.me()));
    assert!(linked.linked_ms.is_some());
    assert!(linked.has_no_activity_seen, "it hasn't written anything yet");
    assert_eq!(linked.last_active_ms, None);

    let general = channel_id(&admin, "general");
    phone
        .send_message(&general, "hello from the phone", None)
        .unwrap();
    sync_all(&[&phone, &bob]).await;

    let linked = &bob.my_devices().unwrap()[1];
    assert!(!linked.has_no_activity_seen);
    assert!(linked.last_active_ms.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_one_of_your_devices_shows_it_removing_until_the_admins_app_does() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;

    bob.remove_my_device(&member_id(&phone.me())).await.unwrap();

    assert!(bob.my_devices().unwrap()[1].is_removing);
    bob.remove_my_device(&member_id(&phone.me())).await.unwrap();
    sync_all(&[&bob, &admin]).await;
    bob.sync().await;
    assert_eq!(ids(&bob), [member_id(&bob.me())]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_linked_device_cannot_remove_the_first_device_and_nobody_removes_their_last() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;

    let refused = format!(
        "{:#}",
        phone.remove_my_device(&member_id(&bob.me())).await.unwrap_err()
    );
    assert!(refused.contains("Remove this one from that device"), "{refused}");
    let both = [member_id(&bob.me()), member_id(&phone.me())];
    let refused = format!(
        "{:#}",
        bob.remove_my_device_and_its_links(&both[0]).await.unwrap_err()
    );
    assert!(refused.contains("at least one"), "{refused}");
    let refused = format!(
        "{:#}",
        admin.remove_my_device(&member_id(&admin.me())).await.unwrap_err()
    );
    assert!(refused.contains("admin's own device"), "{refused}");
}

#[tokio::test(flavor = "multi_thread")]
async fn this_device_can_ask_to_be_removed_when_you_have_another() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;

    phone.remove_my_device(&member_id(&phone.me())).await.unwrap();

    admin.sync().await;
    assert!(!admin.state().mls.is_member(&phone.me()).unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_device_and_its_links_takes_the_devices_it_linked_too() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let tablet = link_device(&admin, &phone, &root.path().join("tablet"), &bucket).await;
    sync_all(&[&admin, &bob, &phone, &tablet]).await;
    assert_eq!(
        bob.my_devices().unwrap()[1].linked_devices,
        [member_id(&tablet.me())]
    );

    bob.remove_my_device_and_its_links(&member_id(&phone.me()))
        .await
        .unwrap();

    sync_all(&[&bob, &admin]).await;
    for device in [phone.me(), tablet.me()] {
        assert!(!admin.state().mls.is_member(&device).unwrap());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn someone_elses_device_is_not_yours_to_remove() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &admin, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;

    let refused = format!(
        "{:#}",
        bob.remove_my_device(&member_id(&phone.me())).await.unwrap_err()
    );

    assert!(refused.contains("isn't one of your devices"), "{refused}");
}

/// Removing this device needs the bucket first: when it can't be reached,
/// nothing is written and the device keeps everything.
#[tokio::test(flavor = "multi_thread")]
async fn removing_this_device_when_the_bucket_is_out_of_reach_changes_nothing() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let general = channel_id(&admin, "general");
    phone.send_message(&general, "waiting to go out", None).unwrap();
    *phone.inner.bucket.write().unwrap() =
        crate::transport::bucket::read_only::ReadingBucket::over(bucket.clone()).into_bucket();
    let written = phone.state().store.last_seq(&phone.me()).unwrap();

    let refused = format!(
        "{:#}",
        phone.remove_my_device(&member_id(&phone.me())).await.unwrap_err()
    );

    assert!(refused.contains("Couldn't reach your workspace"), "{refused}");
    assert_eq!(phone.state().store.last_seq(&phone.me()).unwrap(), written);
    assert!(!phone.my_devices().unwrap()[0].is_removing);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_device_shows_a_notice_on_your_other_devices_only() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let tablet = link_device(&admin, &bob, &root.path().join("tablet"), &bucket).await;
    sync_all(&[&admin, &bob, &phone, &tablet]).await;

    bob.remove_my_device(&member_id(&tablet.me())).await.unwrap();
    sync_all(&[&bob, &admin]).await;
    sync_all(&[&bob, &phone]).await;

    for member in [&bob, &phone] {
        let notices = member.device_notices().unwrap();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].device, member_id(&tablet.me()));
        assert_eq!(notices[0].kind, crate::views::DeviceNoticeKind::Removed);
    }
    assert!(admin.device_notices().unwrap().is_empty());
}

/// The admin's app follows no commit of its own: a removal it commits notes
/// the notice on the admin's device as it commits.
#[tokio::test(flavor = "multi_thread")]
async fn the_admins_own_removal_shows_a_notice_on_the_admins_device() {
    let (root, bucket, admin, _bob) = admin_and_bob().await;
    let phone = link_device(&admin, &admin, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &phone]).await;

    admin.remove_devices(&[member_id(&phone.me())]).await.unwrap();

    let notices = admin.device_notices().unwrap();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].device, member_id(&phone.me()));
    assert_eq!(notices[0].kind, crate::views::DeviceNoticeKind::Removed);
}

/// A stolen phone links a tablet, and the two ask to remove each other: the
/// admin's app removes one and skips the other's request, its author gone.
/// The one left isn't shown as being removed, and the first device's own
/// request is written and takes it.
#[tokio::test(flavor = "multi_thread")]
async fn a_skipped_removal_leaves_no_device_stuck_removing() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let tablet = link_device(&admin, &phone, &root.path().join("tablet"), &bucket).await;
    sync_all(&[&admin, &bob, &phone, &tablet]).await;
    stuck_after_removing_each_other(&admin, &bob, &phone, &tablet).await;
}

/// The same with no thief: two linked devices remove each other in one pass.
#[tokio::test(flavor = "multi_thread")]
async fn two_linked_devices_removing_each_other_leave_none_stuck_removing() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let tablet = link_device(&admin, &bob, &root.path().join("tablet"), &bucket).await;
    sync_all(&[&admin, &bob, &phone, &tablet]).await;
    stuck_after_removing_each_other(&admin, &bob, &phone, &tablet).await;
}

async fn stuck_after_removing_each_other(
    admin: &Workspace,
    bob: &Workspace,
    phone: &Workspace,
    tablet: &Workspace,
) {
    phone.remove_my_device(&member_id(&tablet.me())).await.unwrap();
    tablet.remove_my_device(&member_id(&phone.me())).await.unwrap();
    sync_all(&[phone, tablet]).await;
    admin.sync().await;
    bob.sync().await;
    let left: Vec<_> = [phone, tablet]
        .into_iter()
        .filter(|device| admin.state().mls.is_member(&device.me()).unwrap())
        .collect();
    assert_eq!(left.len(), 1, "one of the two is removed");
    let left = left[0];
    let shown = bob
        .my_devices()
        .unwrap()
        .into_iter()
        .find(|device| device.id == member_id(&left.me()))
        .unwrap();
    assert!(!shown.is_removing, "the skipped request isn't pending");

    let written = bob.state().store.last_seq(&bob.me()).unwrap();
    bob.remove_my_device(&member_id(&left.me())).await.unwrap();
    assert_eq!(bob.state().store.last_seq(&bob.me()).unwrap(), written + 1);
    sync_all(&[bob, admin]).await;
    assert!(!admin.state().mls.is_member(&left.me()).unwrap());
}

/// Removing a device with the devices it linked follows the links all the
/// way down, and the list names every one of them.
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_device_and_its_links_follows_every_link_down() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    let tablet = link_device(&admin, &phone, &root.path().join("tablet"), &bucket).await;
    sync_all(&[&admin, &bob, &phone, &tablet]).await;
    let watch = link_device(&admin, &tablet, &root.path().join("watch"), &bucket).await;
    sync_all(&[&admin, &bob, &phone, &tablet, &watch]).await;
    let shown = bob
        .my_devices()
        .unwrap()
        .into_iter()
        .find(|device| device.id == member_id(&phone.me()))
        .unwrap();
    assert_eq!(
        shown.linked_devices,
        [member_id(&tablet.me()), member_id(&watch.me())]
    );

    bob.remove_my_device_and_its_links(&member_id(&phone.me()))
        .await
        .unwrap();

    sync_all(&[&bob, &admin]).await;
    for device in [phone.me(), tablet.me(), watch.me()] {
        assert!(!admin.state().mls.is_member(&device).unwrap());
    }
}

/// A request written before the epoch moved isn't shown as pending: the
/// admin's app may have skipped it, so asking again writes a new one.
#[tokio::test(flavor = "multi_thread")]
async fn a_request_from_an_earlier_epoch_is_not_shown_removing() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    sync_all(&[&admin, &bob, &phone]).await;
    bob.remove_my_device(&member_id(&phone.me())).await.unwrap();
    admin.commit_keys_only().await.unwrap();

    bob.sync().await;

    assert!(!bob.my_devices().unwrap()[1].is_removing);
    let written = bob.state().store.last_seq(&bob.me()).unwrap();
    bob.remove_my_device(&member_id(&phone.me())).await.unwrap();
    assert_eq!(bob.state().store.last_seq(&bob.me()).unwrap(), written + 1);
}
