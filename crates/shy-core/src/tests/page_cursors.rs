//! Seeing who else is on a page and where their cursors are.

use super::support::*;
use crate::{
    Workspace,
    ops::now_ms,
    pages::cursors::{CursorSelection, CursorUpdate, PageBeacon},
    transport::page_gossip::PageWire,
};

pub(super) fn beacon(from: &Workspace, page: &str, is_open: bool) -> PageWire {
    PageWire::Beacon(
        PageBeacon::signed(
            &from.inner.identity,
            &from.inner.config.workspace_id,
            page,
            is_open,
            now_ms().unwrap(),
        )
        .unwrap(),
    )
}

pub(super) fn cursor(from: &Workspace, page: &str, at: u8) -> PageWire {
    PageWire::Cursor(
        CursorUpdate::signed(
            &from.inner.identity,
            &from.inner.config.workspace_id,
            page,
            Some(CursorSelection {
                anchor: vec![at],
                head: vec![at],
            }),
            now_ms().unwrap(),
        )
        .unwrap(),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn someone_on_the_page_you_have_open_shows_with_their_cursor() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = admin.create_page("Plans", None).unwrap();
    let other = admin.create_page("Notes", None).unwrap();
    admin.join_page(&page, true).await.unwrap();

    admin.receive_page(&beacon(&bob, &page, true)).unwrap();
    admin.receive_page(&cursor(&bob, &page, 3)).unwrap();

    let presence = admin.page_presence(&page).unwrap();
    assert_eq!(presence.viewers, [bob.my_id()]);
    assert_eq!(presence.cursors.len(), 1);
    assert_eq!(presence.cursors[0].member, bob.my_id());
    assert_eq!(presence.cursors[0].selection.as_ref().unwrap().head, [3]);
    assert!(admin.page_presence(&other).unwrap().viewers.is_empty());

    admin.receive_page(&beacon(&bob, &other, true)).unwrap();
    assert!(
        admin.page_presence(&other).unwrap().viewers.is_empty(),
        "nothing is kept about pages you don't have open"
    );

    admin.leave_page(&page).await.unwrap();
    assert!(admin.page_presence(&page).unwrap().viewers.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cursor_is_shared_only_on_the_page_you_joined_sharing() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let page = admin.create_page("Plans", None).unwrap();
    let selection = || {
        Some(CursorSelection {
            anchor: vec![1],
            head: vec![1],
        })
    };

    assert!(admin.move_cursor(&page, selection()).await.is_err());
    admin.join_page(&page, false).await.unwrap();
    assert!(
        admin.move_cursor(&page, selection()).await.is_err(),
        "joined without sharing, nothing is sent"
    );
    admin.join_page(&page, true).await.unwrap();
    admin.move_cursor(&page, selection()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_page_that_exists_can_be_joined() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    assert!(admin.join_page("not-a-page", true).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_who_leaves_the_page_goes_from_it() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = admin.create_page("Plans", None).unwrap();
    admin.join_page(&page, true).await.unwrap();
    admin.receive_page(&beacon(&bob, &page, true)).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    admin.receive_page(&beacon(&bob, &page, false)).unwrap();

    assert!(admin.page_presence(&page).unwrap().viewers.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_sharing_on_or_off_keeps_who_is_on_the_page() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = admin.create_page("Plans", None).unwrap();
    admin.join_page(&page, true).await.unwrap();
    admin.receive_page(&beacon(&bob, &page, true)).unwrap();

    admin.join_page(&page, false).await.unwrap();
    assert_eq!(admin.page_presence(&page).unwrap().viewers, [bob.my_id()]);

    let other = admin.create_page("Notes", None).unwrap();
    admin.join_page(&other, true).await.unwrap();
    admin.join_page(&page, true).await.unwrap();
    assert!(
        admin.page_presence(&page).unwrap().viewers.is_empty(),
        "opening another page forgets this one"
    );
}

fn sent(workspace: &Workspace) -> Vec<PageWire> {
    workspace.inner.page_sent.lock().unwrap().clone()
}

fn is_beacon(message: &PageWire, page: &str, is_open: bool) -> bool {
    matches!(message, PageWire::Beacon(beacon) if beacon.page == page && beacon.is_open == is_open)
}

#[tokio::test(flavor = "multi_thread")]
async fn opening_another_page_says_you_left_the_one_you_shared() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let page = admin.create_page("Plans", None).unwrap();
    let other = admin.create_page("Notes", None).unwrap();
    admin.join_page(&page, true).await.unwrap();

    admin.join_page(&other, true).await.unwrap();

    assert!(
        sent(&admin)
            .iter()
            .any(|message| is_beacon(message, &page, false))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn newcomers_are_greeted_with_your_cursor_at_most_once_a_second() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let page = admin.create_page("Plans", None).unwrap();
    admin.join_page(&page, true).await.unwrap();
    let selection = Some(CursorSelection {
        anchor: vec![4],
        head: vec![4],
    });
    admin.move_cursor(&page, selection.clone()).await.unwrap();
    let before = sent(&admin).len();

    admin.receive_page(&beacon(&bob, &page, true)).unwrap();
    admin.receive_page(&beacon(&carol, &page, true)).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let greetings = &sent(&admin)[before..];
    assert_eq!(
        greetings.len(),
        2,
        "one beacon and one cursor, for both newcomers: {greetings:?}"
    );
    assert!(is_beacon(&greetings[0], &page, true));
    assert!(matches!(&greetings[1], PageWire::Cursor(update) if update.selection == selection));
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_sharing_on_or_off_tells_the_others_at_once() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let page = admin.create_page("Plans", None).unwrap();
    admin.join_page(&page, false).await.unwrap();

    admin.join_page(&page, true).await.unwrap();
    assert!(
        sent(&admin)
            .last()
            .is_some_and(|message| is_beacon(message, &page, true))
    );
    admin.join_page(&page, false).await.unwrap();
    assert!(
        sent(&admin)
            .last()
            .is_some_and(|message| is_beacon(message, &page, false))
    );
    assert!(admin.move_cursor(&page, None).await.is_err(), "no longer sharing");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_on_a_page_from_two_devices_shows_once_and_not_to_themselves() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let page = admin.create_page("Plans", None).unwrap();
    sync_all(&[&admin, &bob, &phone]).await;
    admin.join_page(&page, true).await.unwrap();
    bob.join_page(&page, true).await.unwrap();

    for device in [&bob, &phone] {
        admin.receive_page(&beacon(device, &page, true)).unwrap();
        admin.receive_page(&cursor(device, &page, 3)).unwrap();
    }
    bob.receive_page(&beacon(&phone, &page, true)).unwrap();
    bob.receive_page(&cursor(&phone, &page, 4)).unwrap();

    let seen_by_admin = admin.page_presence(&page).unwrap();
    assert_eq!(seen_by_admin.viewers, [bob.my_id()]);
    assert!(
        seen_by_admin
            .cursors
            .iter()
            .all(|cursor| cursor.member == bob.my_id())
    );
    let seen_by_bob = bob.page_presence(&page).unwrap();
    assert!(seen_by_bob.viewers.is_empty(), "{:?}", seen_by_bob.viewers);
    assert!(seen_by_bob.cursors.is_empty());
}
