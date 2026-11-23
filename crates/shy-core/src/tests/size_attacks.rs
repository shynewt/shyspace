//! Ops too large for their kind: refused without keeping their content, and
//! never holding up their author's later ops.

use super::support::*;
use crate::{
    Workspace,
    ops::{Envelope, MAX_ID_CHARS, MAX_MESSAGE_CHARS, MAX_OP_BYTES, MAX_PAGE_UPDATE_BYTES, Op, new_owned_id},
    pages::Edit,
    private::{
        crypto::{CHANNEL_KEY_LEN, Granting, SealedPayload, new_sealing_key_pair, wrap_key},
        ids::MAX_DM_MEMBERS,
        ops::{GrantBody, MAX_WRAPS_PER_GRANT},
    },
};

fn message(author: &Workspace, channel: &str, text: String) -> Op {
    Op::Message {
        id: new_owned_id(&author.me()),
        channel: channel.to_owned(),
        text,
        reply_to: None,
    }
}

/// The envelope a member keeps for `envelope`'s place in its author's log.
fn kept(member: &Workspace, envelope: &Envelope) -> Envelope {
    member
        .state()
        .store
        .envelopes_after(&envelope.author, envelope.seq - 1)
        .unwrap()
        .into_iter()
        .next()
        .expect("the op is recorded as handled")
}

#[tokio::test(flavor = "multi_thread")]
async fn an_oversized_op_is_refused_without_keeping_it_and_the_next_one_arrives() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let mut forger = Forger::new(&bob);
    let huge = forger.seal(message(&bob, &general, "x".repeat(MAX_OP_BYTES)));
    let next = forger.seal(message(&bob, &general, "after the huge one".into()));

    admin.receive(huge.clone()).unwrap();
    admin.receive(next).unwrap();

    let texts = texts_in(&admin, &general);
    assert!(
        texts.contains(&"after the huge one".to_string()),
        "the author's next op isn't held up"
    );
    assert!(
        !texts.iter().any(|text| text.starts_with("xxx")),
        "the oversized message isn't shown"
    );
    assert!(
        kept(&admin, &huge).ciphertext.is_empty(),
        "its content isn't kept"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_oversized_private_op_is_refused_before_it_is_opened() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let mut forger = Forger::new(&bob);
    let huge = forger.seal(Op::Sealed {
        payload: SealedPayload {
            hint: [0; 8],
            nonce: [0; 24],
            ciphertext: vec![0; MAX_OP_BYTES],
        },
    });

    admin.receive(huge.clone()).unwrap();

    assert!(kept(&admin, &huge).ciphertext.is_empty());
    assert!(
        admin.state().store.unmatched_sealed().unwrap().is_empty(),
        "it doesn't wait for a key"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_oversized_op_arriving_early_isnt_kept_until_its_turn() {
    let (_root, bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let mut forger = Forger::new(&bob);
    let first = forger.seal(message(&bob, &general, "first".into()));
    let huge = forger.seal(message(&bob, &general, "x".repeat(MAX_OP_BYTES)));
    let last = forger.seal(message(&bob, &general, "last".into()));
    // The bucket gets the later two first; the one before them is late.
    bucket.put_segment(&[huge.clone(), last.clone()]).await.unwrap();

    admin.sync().await;
    let parked: Vec<u64> = admin
        .state()
        .store
        .pending()
        .unwrap()
        .iter()
        .map(|envelope| envelope.seq)
        .collect();
    assert_eq!(
        parked,
        vec![last.seq],
        "the oversized op isn't kept while it waits"
    );

    admin.receive(first).unwrap();
    // Gap repair notices the waiting op's missing predecessor and reads
    // the author's history again.
    admin.sync().await;
    admin.sync().await;

    assert!(kept(&admin, &huge).ciphertext.is_empty());
    assert!(texts_in(&admin, &general).contains(&"last".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_op_too_large_for_any_kind_is_never_parked() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let mut huge = Forger::new(&bob).seal(Op::Delete { id: "x".into() });
    // For an epoch whose key hasn't arrived, so it can't be checked yet.
    huge.epoch += 1;
    huge.ciphertext = vec![0; MAX_PAGE_UPDATE_BYTES + 1];

    assert!(admin.receive(huge).is_err());
    assert!(admin.state().store.pending().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_longest_message_in_the_widest_characters_still_fits() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let longest = "𝄞".repeat(MAX_MESSAGE_CHARS);

    admin.send_message(&general, &longest, None).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert!(texts_in(&bob, &general).contains(&longest));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_oversized_page_update_is_refused_and_later_edits_still_apply() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = admin.create_page("Notes", None).unwrap();
    sync_all(&[&admin, &bob]).await;
    let mut forger = Forger::new(&bob);
    let huge = forger.seal(page_update(
        &bob,
        &page,
        None,
        Edit::Append(&"y".repeat(MAX_PAGE_UPDATE_BYTES)),
    ));
    let small = forger.seal(page_update(&bob, &page, None, Edit::Append("small edit")));

    admin.receive(huge.clone()).unwrap();
    admin.receive(small).unwrap();

    assert_eq!(admin.page_text(&page).unwrap(), "small edit");
    assert!(kept(&admin, &huge).ciphertext.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_large_page_paste_within_the_limit_reaches_everyone() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let page = admin.create_page("Book", None).unwrap();
    let book = "a long chapter. ".repeat(MAX_PAGE_UPDATE_BYTES / 20);

    admin.edit_page(&page, Edit::Append(&book)).unwrap();
    sync_all(&[&admin, &bob]).await;

    assert_eq!(bob.page_text(&page).unwrap().len(), book.len());
}

#[tokio::test(flavor = "multi_thread")]
async fn your_own_oversized_page_paste_is_refused_with_a_clear_error() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let page = admin.create_page("Notes", None).unwrap();

    let error = admin
        .edit_page(&page, Edit::Append(&"y".repeat(MAX_PAGE_UPDATE_BYTES)))
        .unwrap_err();

    assert!(error.to_string().contains("too large"), "{error:#}");
    assert_eq!(admin.page_text(&page).unwrap(), "", "nothing was saved");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_grant_to_the_most_people_fits_the_limit() {
    let (_root, _bucket, admin, _bob) = admin_and_bob().await;
    let recipient = new_sealing_key_pair().unwrap();
    let granting = Granting {
        workspace_id: "workspace",
        granter: &admin.me(),
    };
    let grant_of = |dm_members: Vec<_>| {
        postcard::to_stdvec(&GrantBody {
            channel: "p".repeat(MAX_ID_CHARS),
            generation: u64::MAX,
            key: [0xff; CHANNEL_KEY_LEN],
            dm_members,
        })
        .unwrap()
    };
    // A channel's grant names no members but can reach the most people; a
    // DM's names its members but reaches only them.
    let largest = [
        (grant_of(Vec::new()), MAX_WRAPS_PER_GRANT),
        (grant_of(vec![admin.me(); MAX_DM_MEMBERS]), MAX_DM_MEMBERS),
    ];
    let mut forger = Forger::new(&admin);
    for (grant, recipients) in largest {
        let wraps = (0..recipients)
            .map(|_| wrap_key(granting, &recipient.public, &grant).unwrap())
            .collect();
        let op = Op::KeyGrant { wraps };
        assert!(forger.seal(op.clone()).fits_size_limit(&op));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn padding_someone_elses_op_past_the_limit_doesnt_get_it_refused() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let general = channel_id(&admin, "general");
    let real = Forger::new(&bob).seal(message(&bob, &general, "the real message".into()));
    let (_, epoch_key) = current_key(&admin);

    // Another member re-encrypts Bob's op with padding and gets it to the
    // admin first.
    assert!(
        admin
            .receive(encrypted_again(&real, &epoch_key, MAX_OP_BYTES))
            .is_err()
    );
    admin.receive(real).unwrap();
    assert_eq!(texts_in(&admin, &general), ["the real message"]);
}
