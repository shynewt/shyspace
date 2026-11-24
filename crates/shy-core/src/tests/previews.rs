//! Previews that reach the app as plain text (thread summaries, search
//! snippets and notifications) name every app link by its real target, as
//! a chip does: the member's current name, the page's title after 📄, "💬 a
//! message". An author's label never shows, even for a reference link whose
//! definition a cut left out.

use super::support::*;
use crate::views::Change;

fn last_reply_preview(member: &crate::Workspace, channel: &str, root: &str) -> String {
    member
        .messages(channel, None, 50)
        .unwrap()
        .into_iter()
        .find(|message| message.id == root)
        .and_then(|message| message.last_reply_preview)
        .expect("a thread summary")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_thread_summary_names_the_member_a_mention_really_links_to() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "who?", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let lying = format!("[@Ada](shy://member/{}) knows", carol.my_id());
    bob.send_message(&general, &lying, Some(question.clone()))
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    assert_eq!(last_reply_preview(&admin, &general, &question), "@Carol knows");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_thread_summary_names_a_reference_link_defined_past_its_cut() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "who?", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let text = format!(
        "[@Ada] {}\n\n[@Ada]: shy://member/{}",
        "word ".repeat(40),
        carol.my_id()
    );
    bob.send_message(&general, &text, Some(question.clone())).unwrap();
    sync_all(&[&admin, &bob]).await;

    let preview = last_reply_preview(&admin, &general, &question);
    assert!(preview.starts_with("@Carol word word"), "{preview}");
    assert!(
        !preview.contains("Ada") && !preview.contains("shy://"),
        "{preview}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn page_message_and_unknown_member_links_are_named_like_chips() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let page = admin.create_page("Handbook", None).unwrap();
    let question = admin.send_message(&general, "where?", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let stranger = "1a2b3c".to_owned() + &"0".repeat(58);
    let text = format!(
        "see [notes](shy://page/{page}), [this](shy://message/{question}) and [@Zed](shy://member/{stranger}); also shy://page/{page}"
    );
    bob.send_message(&general, &text, Some(question.clone())).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert_eq!(
        last_reply_preview(&admin, &general, &question),
        "see 📄 Handbook, 💬 a message and @member 1a2b3c; also 📄 Handbook"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_notification_names_the_member_a_mention_really_links_to() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let mut changes = admin.subscribe();
    let lying = format!("[@Bob](shy://member/{}) look", admin.my_id());
    bob.send_message(&general, &lying, None).unwrap();
    sync_all(&[&admin, &bob]).await;

    let mut previews = Vec::new();
    while let Ok(change) = changes.try_recv() {
        if let Change::Alert { preview, .. } = change {
            previews.push(preview);
        }
    }
    assert_eq!(previews, ["@Ada look"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_search_snippet_names_links_and_marks_the_match() {
    let (root, bucket, admin, bob) = admin_and_bob().await;
    let carol = join(&admin, &root.path().join("carol"), &bucket, "Carol", false).await;
    sync_all(&[&admin, &bob, &carol]).await;
    let general = channel_id(&admin, "general");
    let page = admin.create_page("Handbook", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let text = format!(
        "**budget** by [@Ada](shy://member/{}) in [doc](shy://page/{page})",
        carol.my_id()
    );
    bob.send_message(&general, &text, None).unwrap();
    sync_all(&[&admin, &bob]).await;

    let hits = admin.search("budg", 10).unwrap();
    let snippet = &hits.iter().find(|hit| hit.kind == "message").unwrap().snippet;
    assert_eq!(snippet, "\u{1}budget\u{2} by @Carol in 📄 Handbook");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_search_snippet_is_cut_around_the_match() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let text = format!("{} the needle {}", "before ".repeat(30), "after ".repeat(30));
    bob.send_message(&general, &text, None).unwrap();
    sync_all(&[&admin, &bob]).await;

    let snippet = &admin.search("needle", 10).unwrap()[0].snippet;
    assert!(snippet.starts_with('…') && snippet.ends_with('…'), "{snippet}");
    assert!(snippet.contains("the \u{1}needle\u{2} after"), "{snippet}");
    assert!(snippet.split_whitespace().count() <= 14, "{snippet}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_search_by_filters_alone_shows_plain_named_text() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let text = format!("_hi_ [@Bob](shy://member/{})", admin.my_id());
    bob.send_message(&general, &text, None).unwrap();
    sync_all(&[&admin, &bob]).await;

    let hits = admin.search("from:bob", 10).unwrap();
    assert_eq!(hits[0].snippet, "hi @Ada");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_cant_fake_a_highlight_in_its_snippet() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    bob.send_message(&general, "\u{1}fake\u{2} budget", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    let snippet = &admin.search("budget", 10).unwrap()[0].snippet;
    assert_eq!(snippet, "fake \u{1}budget\u{2}");
}

/// A link's icon is a word of its own, so a match right after it is marked
/// whole and the cut still counts words, whatever their bytes.
#[tokio::test(flavor = "multi_thread")]
async fn a_match_next_to_a_named_link_is_marked_exactly() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let page = admin.create_page("Handbook", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let text = format!(
        "{} see shy://page/{page} budget shy://page/{page} {}",
        "before ".repeat(10),
        "after ".repeat(10)
    );
    bob.send_message(&general, &text, None).unwrap();
    sync_all(&[&admin, &bob]).await;

    let snippet = &admin.search("budget", 10).unwrap()[0].snippet;
    assert_eq!(
        snippet,
        "…before see 📄 Handbook \u{1}budget\u{2} 📄 Handbook after after after after after…"
    );
}
