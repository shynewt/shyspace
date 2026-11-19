//! Search with filters: from:, in:, before:, after:.

use super::support::*;

fn found(workspace: &crate::Workspace, query: &str) -> Vec<String> {
    let mut snippets: Vec<String> = workspace
        .search(query, 20)
        .unwrap()
        .into_iter()
        .map(|hit| hit.snippet.replace(['\u{1}', '\u{2}'], ""))
        .collect();
    snippets.sort();
    snippets
}

#[tokio::test(flavor = "multi_thread")]
async fn filters_narrow_search_by_author_channel_and_date() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let random = bob.create_channel("random").unwrap();
    admin
        .send_message(&general, "deploy notes from ada", None)
        .unwrap();
    bob.send_message(&general, "deploy went fine", None).unwrap();
    bob.send_message(&random, "deploy party tonight", None).unwrap();
    admin.create_page("Deploy checklist", None).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert_eq!(found(&admin, "deploy").len(), 4, "messages and pages");
    assert_eq!(
        found(&admin, "deploy from:bob"),
        vec!["deploy party tonight", "deploy went fine"]
    );
    assert_eq!(found(&admin, "deploy from:me"), vec!["deploy notes from ada"]);
    assert_eq!(found(&admin, "deploy in:#random"), vec!["deploy party tonight"]);
    assert_eq!(
        found(&admin, "from:Bob in:general"),
        vec!["deploy went fine"],
        "filters alone list messages"
    );
    assert!(found(&admin, "deploy before:2000-01-01").is_empty());
    assert_eq!(
        found(&admin, "deploy after:2000-01-01").len(),
        3,
        "dates apply to messages only"
    );
    assert!(found(&admin, "from:nobody").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reply_found_by_search_knows_its_thread() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let question = admin.send_message(&general, "which version?", None).unwrap();
    bob.send_message(&general, "version two", Some(question.clone()))
        .unwrap();
    sync_all(&[&admin, &bob]).await;

    let hit = admin.search("two", 5).unwrap().pop().unwrap();
    assert_eq!(hit.thread_root.as_deref(), Some(question.as_str()));
}
