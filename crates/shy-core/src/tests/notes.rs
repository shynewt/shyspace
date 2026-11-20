//! Notes to yourself: a DM whose only member is you.

use super::support::*;
use crate::{Workspace, views::ChannelKind};

fn texts(member: &Workspace, channel: &str) -> Vec<String> {
    member
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn notes_are_yours_alone_and_opening_them_again_finds_the_same_ones() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let notes = bob.open_notes().unwrap();
    bob.send_message(&notes, "buy milk", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert_eq!(bob.open_notes().unwrap(), notes);
    let listed = bob
        .channels()
        .unwrap()
        .into_iter()
        .find(|channel| channel.id == notes)
        .unwrap();
    assert_eq!(listed.kind, ChannelKind::Notes);
    assert!(listed.dm_members.is_empty());
    assert_eq!(listed.unread, 0, "you wrote it");
    assert_eq!(texts(&bob, &notes), ["buy milk"]);
    assert_eq!(bob.search("milk", 10).unwrap().len(), 1);

    assert!(
        admin
            .channels()
            .unwrap()
            .iter()
            .all(|channel| channel.id != notes)
    );
    assert!(texts(&admin, &notes).is_empty());
    assert!(admin.search("milk", 10).unwrap().is_empty());
}
