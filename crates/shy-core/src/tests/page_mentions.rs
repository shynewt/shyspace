//! Mentions of you that others add to pages: they wait in the inbox, and a
//! recent one alerts, like a chat mention.

use super::support::*;
use crate::{
    crypto,
    links::app_link,
    ops::{OpBody, now_ms},
    pages::Edit,
    views::{Change, NotifyLevel, StatusUpdate},
    workspace::Workspace,
};

/// A mention of `whom`, as the desktop editor writes it.
fn mention_of(whom: &Workspace) -> String {
    format!(
        "[@{}]({})",
        "Ada",
        app_link(&whom.inner.config.workspace_id, "member", &whom.my_id())
    )
}

fn page_alerts(changes: &mut tokio::sync::broadcast::Receiver<Change>) -> Vec<String> {
    let mut previews = Vec::new();
    while let Ok(change) = changes.try_recv() {
        if let Change::PageAlert { preview, .. } = change {
            previews.push(preview);
        }
    }
    previews
}

fn inbox_previews(member: &Workspace) -> Vec<String> {
    member
        .inbox()
        .unwrap()
        .page_mentions
        .into_iter()
        .map(|mention| mention.preview)
        .collect()
}

async fn shared_page(admin: &Workspace, bob: &Workspace, text: &str) -> String {
    let page = bob.create_page("Plan", None).unwrap();
    bob.edit_page(&page, Edit::Append(text)).unwrap();
    sync_all(&[admin, bob]).await;
    page
}

/// Seals `bob`'s edit of `page`, written at `timestamp_ms` by his clock.
fn edit_written_at(bob: &Workspace, page: &str, text: &str, timestamp_ms: i64) -> crate::ops::Envelope {
    let state = bob.page_state(page).unwrap();
    let (epoch, key) = current_key(bob);
    let seq = bob.state().store.next_seq(&bob.me()).unwrap();
    let body = OpBody {
        lamport: 9_000,
        timestamp_ms,
        op: page_update(bob, page, state.as_deref(), Edit::Append(text)),
    };
    crypto::seal(&bob.inner.identity, &key, epoch, seq, &body).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_mention_in_a_page_alerts_and_waits_in_the_inbox() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = shared_page(&admin, &bob, "# Plan").await;
    let mut changes = admin.subscribe();

    bob.edit_page(
        &page,
        Edit::Append(&format!("Ask {} about keys", mention_of(&admin))),
    )
    .unwrap();
    sync_all(&[&admin, &bob]).await;

    let name = admin
        .members()
        .unwrap()
        .into_iter()
        .find(|m| m.id == admin.my_id())
        .unwrap()
        .name;
    let line = format!("Ask @{name} about keys");
    assert_eq!(page_alerts(&mut changes), vec![line.clone()]);
    let inbox = admin.inbox().unwrap();
    let [mention] = inbox.page_mentions.as_slice() else {
        panic!("one page mention: {:?}", inbox.page_mentions);
    };
    assert_eq!(mention.page, page);
    assert_eq!(mention.page_title, "Plan");
    assert_eq!(mention.author, bob.my_id());
    assert_eq!(mention.preview, line);

    admin.mark_page_mentions_read(&page).unwrap();
    assert!(inbox_previews(&admin).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mention_already_on_a_page_never_counts_again() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = shared_page(&admin, &bob, &format!("Ask {}", mention_of(&admin))).await;
    admin.mark_page_mentions_read(&page).unwrap();
    let mut changes = admin.subscribe();

    bob.edit_page(&page, Edit::Append("more notes")).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert!(page_alerts(&mut changes).is_empty());
    assert!(inbox_previews(&admin).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn your_own_mentions_and_everyone_mentions_in_pages_count_for_nobody() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = shared_page(&admin, &bob, "# Plan").await;
    let mut admin_changes = admin.subscribe();
    let mut bob_changes = bob.subscribe();
    let workspace_id = &admin.inner.config.workspace_id;

    admin
        .edit_page(
            &page,
            Edit::Append(&format!("Note to self {}", mention_of(&admin))),
        )
        .unwrap();
    bob.edit_page(
        &page,
        Edit::Append(&format!(
            "[@channel]({}) [@here]({})",
            app_link(workspace_id, "mention", "channel"),
            app_link(workspace_id, "mention", "here")
        )),
    )
    .unwrap();
    sync_all(&[&admin, &bob]).await;

    assert!(page_alerts(&mut admin_changes).is_empty());
    assert!(page_alerts(&mut bob_changes).is_empty());
    assert!(inbox_previews(&admin).is_empty());
    assert!(inbox_previews(&bob).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_status_that_silences_alerts_still_keeps_the_inbox_entry() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = shared_page(&admin, &bob, "# Plan").await;
    admin
        .set_status(StatusUpdate {
            emoji: "🎧",
            text: "Focus",
            until_ms: None,
            alerts: NotifyLevel::Nothing,
        })
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    let mut changes = admin.subscribe();

    bob.edit_page(&page, Edit::Append(&format!("Ask {}", mention_of(&admin))))
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    assert!(page_alerts(&mut changes).is_empty());
    assert_eq!(inbox_previews(&admin).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mention_caught_up_later_waits_in_the_inbox_without_alerting() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = shared_page(&admin, &bob, "# Plan").await;
    let mut changes = admin.subscribe();
    let an_hour_ago = now_ms().unwrap() - 60 * 60 * 1000;

    admin
        .receive(edit_written_at(
            &bob,
            &page,
            &format!("Ask {}", mention_of(&admin)),
            an_hour_ago,
        ))
        .unwrap();

    assert!(page_alerts(&mut changes).is_empty());
    let inbox = admin.inbox().unwrap();
    assert_eq!(inbox.page_mentions.len(), 1);
    assert_eq!(inbox.page_mentions[0].mentioned_ms, an_hour_ago);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mention_from_over_a_week_ago_stays_out_of_the_inbox() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = shared_page(&admin, &bob, "# Plan").await;
    let eight_days_ago = now_ms().unwrap() - 8 * 24 * 60 * 60 * 1000;

    admin
        .receive(edit_written_at(
            &bob,
            &page,
            &format!("Ask {}", mention_of(&admin)),
            eight_days_ago,
        ))
        .unwrap();

    assert!(inbox_previews(&admin).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_inbox_keeps_one_entry_per_page_and_author_at_the_latest_mention() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = shared_page(&admin, &bob, "# Plan").await;

    bob.edit_page(&page, Edit::Append(&format!("First {}", mention_of(&admin))))
        .unwrap();
    bob.edit_page(&page, Edit::Append(&format!("Second {}", mention_of(&admin))))
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let previews = inbox_previews(&admin);
    assert_eq!(previews.len(), 1);
    assert!(previews[0].starts_with("Second "), "{previews:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_mention_keeps_its_entry_and_its_place() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = shared_page(&admin, &bob, "# Plan").await;
    let mention = format!("Ask {}", mention_of(&admin));
    bob.edit_page(&page, Edit::Append(&mention)).unwrap();
    bob.edit_page(
        &page,
        Edit::Replace {
            find: &mention,
            with: "Done",
        },
    )
    .unwrap();
    sync_all(&[&admin, &bob]).await;

    let inbox = admin.inbox().unwrap();
    let [entry] = inbox.page_mentions.as_slice() else {
        panic!("one page mention: {:?}", inbox.page_mentions);
    };
    assert!(entry.preview.starts_with("Ask @"), "{}", entry.preview);
    let state = admin.page_state(&page).unwrap();
    assert_eq!(
        crate::pages::positions_held(state.as_deref(), &[&entry.position]).unwrap(),
        [true]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_the_page_takes_its_mentions_out_of_the_inbox() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = shared_page(&admin, &bob, "# Plan").await;
    bob.edit_page(&page, Edit::Append(&format!("Ask {}", mention_of(&admin))))
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    assert_eq!(inbox_previews(&admin).len(), 1);

    bob.delete_page(&page).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert!(inbox_previews(&admin).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn marking_everything_read_clears_page_mentions_too() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = shared_page(&admin, &bob, "# Plan").await;
    bob.edit_page(&page, Edit::Append(&format!("Ask {}", mention_of(&admin))))
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    admin.mark_all_read().unwrap();

    assert!(inbox_previews(&admin).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_workspace_badge_counts_page_mentions() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = shared_page(&admin, &bob, "# Plan").await;
    let before = admin.badge().unwrap();

    bob.edit_page(&page, Edit::Append(&format!("Ask {}", mention_of(&admin))))
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let badge = admin.badge().unwrap();
    assert!(badge.unread);
    assert_eq!(badge.mentions, before.mentions + 1);
    assert_eq!(badge.attention, before.attention + 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_inbox_shows_the_line_as_it_was_written() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = shared_page(&admin, &bob, "# Plan").await;
    let mention = format!("Ask {} about keys", mention_of(&admin));
    bob.edit_page(&page, Edit::Append(&mention)).unwrap();
    bob.edit_page(
        &page,
        Edit::Replace {
            find: " about keys",
            with: " today",
        },
    )
    .unwrap();
    sync_all(&[&admin, &bob]).await;

    let previews = inbox_previews(&admin);
    assert_eq!(previews.len(), 1);
    assert!(previews[0].ends_with(" about keys"), "{previews:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mention_whose_text_the_page_never_held_is_forgotten_after_a_rebuild() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = shared_page(&admin, &bob, "# Plan").await;
    bob.edit_page(&page, Edit::Append(&format!("Kept {}", mention_of(&admin))))
        .unwrap();
    sync_all(&[&admin, &bob]).await;
    // As if Bob's later edit had been undone with a removed member's late ops.
    let undone = crate::pages::insertions(&page_update_bytes(
        &bob,
        &page,
        bob.page_state(&page).unwrap().as_deref(),
        Edit::Append("never kept"),
    ))
    .unwrap()
    .into_iter()
    .map(|inserted| crate::pages::position_in(&inserted, 0).unwrap())
    .next()
    .unwrap();
    let ghost = [9u8; 32];
    admin
        .state()
        .store
        .note_page_mention(&page, &ghost, &undone, "gone", now_ms().unwrap())
        .unwrap();
    assert_eq!(inbox_previews(&admin).len(), 2);

    let changes = admin.forget_undone_page_mentions(&admin.state()).unwrap();

    assert!(matches!(changes.as_slice(), [Change::PageMentions]));
    let previews = inbox_previews(&admin);
    assert_eq!(previews.len(), 1);
    assert!(previews[0].starts_with("Kept "), "{previews:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_mention_from_a_linked_device_is_its_persons_one_entry() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let page = shared_page(&admin, &bob, "# Plan").await;
    sync_all(&[&admin, &bob, &phone]).await;
    let mut changes = admin.subscribe();

    // Four mentions from Bob's two devices in turn, over his alert bound.
    for (round, device) in [&bob, &phone, &bob, &phone].into_iter().enumerate() {
        device
            .edit_page(
                &page,
                Edit::Append(&format!("Ask {} ({round})", mention_of(&admin))),
            )
            .unwrap();
        sync_all(&[&admin, &bob, &phone]).await;
    }

    let inbox = admin.inbox().unwrap();
    let [mention] = inbox.page_mentions.as_slice() else {
        panic!("one entry for Bob: {:?}", inbox.page_mentions);
    };
    assert_eq!(mention.author, bob.my_id());
    let mut alerts = Vec::new();
    while let Ok(change) = changes.try_recv() {
        if let Change::PageAlert {
            author, author_name, ..
        } = change
        {
            alerts.push((author, author_name));
        }
    }
    assert_eq!(
        alerts,
        vec![(bob.my_id(), "Bob".to_string()); 3],
        "Bob's bound of three"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_mention_from_my_other_device_makes_no_entry_and_no_alert() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let phone = link_device(&admin, &bob, &root.path().join("phone"), &bucket).await;
    let page = shared_page(&admin, &bob, "# Plan").await;
    sync_all(&[&admin, &bob, &phone]).await;
    let mut changes = bob.subscribe();

    phone
        .edit_page(&page, Edit::Append(&format!("Note to {}", mention_of(&bob))))
        .unwrap();
    sync_all(&[&admin, &bob, &phone]).await;

    assert!(inbox_previews(&bob).is_empty());
    assert!(page_alerts(&mut changes).is_empty());
}
