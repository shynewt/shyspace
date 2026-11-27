//! Unread tracking: personal read positions kept on each device.

use super::support::*;

/// Marks everything currently shown in `channel` as read, like the app does.
fn read_all(workspace: &crate::Workspace, channel: &str) {
    if let Some(latest) = workspace.messages(channel, None, 1).unwrap().pop() {
        workspace.mark_read(channel, &latest.id).unwrap();
    }
}

fn unread(workspace: &crate::Workspace, channel: &str) -> u32 {
    workspace
        .channels()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == channel)
        .unwrap()
        .unread
}

#[tokio::test(flavor = "multi_thread")]
async fn others_messages_are_unread_until_the_channel_is_read() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    read_all(&admin, &general);
    bob.send_message(&general, "one", None).unwrap();
    bob.send_message(&general, "two", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert_eq!(unread(&admin, &general), 2);
    read_all(&admin, &general);
    assert_eq!(unread(&admin, &general), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn your_own_messages_are_never_unread_and_sending_marks_the_channel_read() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    read_all(&admin, &general);
    bob.send_message(&general, "question?", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert_eq!(unread(&admin, &general), 1);

    admin.send_message(&general, "answer", None).unwrap();
    assert_eq!(unread(&admin, &general), 0, "replying means you've read it");
    let last_read = admin
        .channels()
        .unwrap()
        .into_iter()
        .find(|c| c.id == general)
        .unwrap()
        .last_read;
    assert_eq!(last_read.unwrap().device, crate::views::member_id(&admin.me()));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_read_position_never_moves_back() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let first = bob.send_message(&general, "first", None).unwrap();
    bob.send_message(&general, "second", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    read_all(&admin, &general);

    admin.mark_read(&general, &first).unwrap();
    assert_eq!(unread(&admin, &general), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_unread_message_stops_counting() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    read_all(&admin, &general);
    let oops = bob.send_message(&general, "oops", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert_eq!(unread(&admin, &general), 1);

    bob.delete_message(&oops).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert_eq!(unread(&admin, &general), 0);
}
