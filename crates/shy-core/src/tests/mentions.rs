//! Mentions: links to a member (or @channel / @here) inside message text.

use super::support::*;
use crate::mentions;

fn channel_view(workspace: &crate::Workspace, channel: &str) -> crate::views::Channel {
    workspace
        .channels()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == channel)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mention_counts_for_the_person_mentioned_only() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob, &carol]).await;
    for member in [&admin, &carol] {
        if let Some(latest) = member.messages(&general, None, 1).unwrap().pop() {
            member.mark_read(&general, &latest.id).unwrap();
        }
    }
    let text = format!("{} can you look?", mentions::member_link("Ada", &admin.my_id()));
    bob.send_message(&general, &text, None).unwrap();
    sync_all(&[&admin, &bob, &carol]).await;

    assert_eq!(channel_view(&admin, &general).unread_mentions, 1);
    assert_eq!(channel_view(&carol, &general).unread_mentions, 0);
    assert_eq!(channel_view(&carol, &general).unread, 1);
    let seen_by_admin = admin.messages(&general, None, 1).unwrap().pop().unwrap();
    assert!(seen_by_admin.mentions_me);
    assert!(
        !carol
            .messages(&general, None, 1)
            .unwrap()
            .pop()
            .unwrap()
            .mentions_me
    );

    admin.mark_read(&general, &seen_by_admin.id).unwrap();
    assert_eq!(channel_view(&admin, &general).unread_mentions, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn at_channel_and_at_here_mention_everyone_but_the_author() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(
        &general,
        &format!("{} standup in 5", mentions::CHANNEL_LINK_MARKDOWN),
        None,
    )
    .unwrap();
    bob.send_message(
        &general,
        &format!("{} anyone around?", mentions::HERE_LINK_MARKDOWN),
        None,
    )
    .unwrap();
    sync_all(&[&admin, &bob]).await;

    let admins: Vec<_> = admin
        .messages(&general, None, 2)
        .unwrap()
        .into_iter()
        .map(|m| m.mentions_me)
        .collect();
    assert_eq!(admins, vec![true, true]);
    let bobs: Vec<_> = bob
        .messages(&general, None, 2)
        .unwrap()
        .into_iter()
        .map(|m| m.mentions_me)
        .collect();
    assert_eq!(bobs, vec![false, false], "your own @channel doesn't mention you");
}

#[tokio::test(flavor = "multi_thread")]
async fn unread_mentions_are_listed_across_channels_once() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let random = bob.create_channel("random").unwrap();
    let mention = mentions::member_link("Ada", &admin.my_id());
    bob.send_message(&general, "no mention here", None).unwrap();
    bob.send_message(&general, &format!("{mention} first"), None)
        .unwrap();
    bob.send_message(&random, &format!("{mention} second"), None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let texts: Vec<_> = admin
        .take_unread_mentions()
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert!(texts.iter().all(|text| text.contains(&mention)));
    assert!(
        admin.take_unread_mentions().unwrap().is_empty(),
        "taking them marks them read"
    );
}

#[test]
fn member_links_keep_names_from_breaking_the_markdown() {
    let link = mentions::member_link("Ada [the] Admin", "ab12");
    assert_eq!(link, "[@Ada the Admin](shy://member/ab12)");
    assert!(mentions::mentions(&link, "0123456789abcdef", "ab12"));
    assert!(
        !mentions::mentions(&link, "0123456789abcdef", "ab1"),
        "a prefix of an id isn't the id"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mention_naming_this_workspace_counts_like_an_older_one() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    sync_all(&[&admin, &bob]).await;
    if let Some(latest) = admin.messages(&general, None, 1).unwrap().pop() {
        admin.mark_read(&general, &latest.id).unwrap();
    }
    let link = crate::links::app_link(admin.workspace_id(), "member", &admin.my_id());
    bob.send_message(&general, &format!("[@Ada]({link}) can you look?"), None)
        .unwrap();
    let everyone = crate::links::app_link(admin.workspace_id(), "mention", "channel");
    bob.send_message(&general, &format!("[@channel]({everyone}) standup"), None)
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    assert_eq!(channel_view(&admin, &general).unread_mentions, 2);
    assert!(
        admin
            .messages(&general, None, 2)
            .unwrap()
            .iter()
            .all(|message| message.mentions_me)
    );
}
