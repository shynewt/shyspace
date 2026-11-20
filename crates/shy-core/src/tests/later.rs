//! Later: saved messages and pages, with optional reminders. Personal.

use super::support::*;
use crate::ops::now_ms;

#[tokio::test(flavor = "multi_thread")]
async fn saved_items_are_listed_with_what_they_point_to() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let message = bob.send_message(&general, "read this later", None).unwrap();
    let page = bob.create_page("Spec", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    admin.save_for_later(&message, None).unwrap();
    admin.save_for_later(&page, None).unwrap();
    let later = admin.later().unwrap();
    assert_eq!(later.len(), 2);
    assert_eq!(
        later[0].page.as_ref().map(|p| p.title.as_str()),
        Some("Spec"),
        "newest first"
    );
    assert_eq!(
        later[1].message.as_ref().map(|m| m.message.text.as_str()),
        Some("read this later")
    );
    assert!(
        admin
            .messages(&general, None, 50)
            .unwrap()
            .iter()
            .any(|m| m.id == message && m.saved.is_some())
    );
    assert!(bob.later().unwrap().is_empty(), "saved items stay on the device");

    admin.mark_later_done(&message, true).unwrap();
    assert!(
        admin
            .later()
            .unwrap()
            .iter()
            .any(|item| item.id == message && item.is_done)
    );
    admin.remove_from_later(&page).unwrap();
    assert_eq!(admin.later().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_message_leaves_later() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let message = bob.send_message(&general, "oops", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    admin.save_for_later(&message, None).unwrap();

    bob.delete_message(&message).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(admin.later().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn due_reminders_are_handed_out_once() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let soon = admin.send_message(&general, "call the bank", None).unwrap();
    let later_on = admin.send_message(&general, "next week", None).unwrap();
    let now = now_ms().unwrap();
    admin.save_for_later(&soon, Some(now - 1_000)).unwrap();
    admin.save_for_later(&later_on, Some(now + 3_600_000)).unwrap();

    let due: Vec<_> = admin
        .take_due_reminders()
        .unwrap()
        .into_iter()
        .map(|item| item.id)
        .collect();
    assert_eq!(due, vec![soon.clone()]);
    assert!(
        admin.take_due_reminders().unwrap().is_empty(),
        "each reminder fires once"
    );
    admin.save_for_later(&soon, Some(now - 500)).unwrap();
    assert_eq!(
        admin.take_due_reminders().unwrap().len(),
        1,
        "a new reminder time fires again"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_carries_its_reminder_and_whether_it_is_done() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let message = bob.send_message(&general, "call back", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let saved_mark = || {
        admin
            .messages(&general, None, 50)
            .unwrap()
            .into_iter()
            .find(|m| m.id == message)
            .unwrap()
            .saved
    };
    assert_eq!(saved_mark(), None);

    let at = now_ms().unwrap() + 60_000;
    admin.save_for_later(&message, Some(at)).unwrap();
    assert_eq!(
        saved_mark(),
        Some(crate::views::SavedMark {
            remind_ms: Some(at),
            is_done: false
        })
    );
    admin.mark_later_done(&message, true).unwrap();
    assert_eq!(
        saved_mark(),
        Some(crate::views::SavedMark {
            remind_ms: Some(at),
            is_done: true
        })
    );
}
