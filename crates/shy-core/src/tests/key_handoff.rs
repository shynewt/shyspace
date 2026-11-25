//! A member who missed a bucket key change while offline gets the new key
//! from another member once the old one is deleted (#259).

use std::sync::Mutex;

use anyhow::{Context, Result};
use iroh::PublicKey;

use super::{
    key_rotation::{FIRST_KEY, Provider, admin_bob_carol, change_key},
    support::*,
};
use crate::{
    Workspace,
    sync::key_handoff::{
        KeyAnswer, KeyAsker, KeyRequest, KeyState, MAX_HANDOFF_COMMIT_BYTES, MAX_HANDOFF_COMMITS,
    },
    views::PresenceState,
};

/// Members `asker` can reach, answering as their app would.
pub struct Peers<'a> {
    asker: PublicKey,
    reachable: Vec<&'a Workspace>,
    max_commits: usize,
    /// Members who answer this, whatever they hold.
    lies: Vec<(PublicKey, KeyAnswer)>,
    asked: Mutex<Vec<PublicKey>>,
}

impl<'a> Peers<'a> {
    pub fn new(asker: &Workspace, reachable: &[&'a Workspace]) -> Self {
        Self {
            asker: asker.me(),
            reachable: reachable.to_vec(),
            max_commits: MAX_HANDOFF_COMMITS,
            lies: Vec::new(),
            asked: Mutex::default(),
        }
    }

    fn times_asked(&self, member: &Workspace) -> usize {
        let asked = self.asked.lock().unwrap();
        asked.iter().filter(|asked| **asked == member.me()).count()
    }
}

impl KeyAsker for Peers<'_> {
    async fn ask(&self, member: PublicKey, request: KeyRequest) -> Result<KeyAnswer> {
        self.asked.lock().unwrap().push(member);
        if let Some((_, lie)) = self.lies.iter().find(|(liar, _)| *liar == member) {
            return Ok(lie.clone());
        }
        let peer = self
            .reachable
            .iter()
            .find(|peer| peer.me() == member)
            .context("not online")?;
        peer.key_answer_within(&self.asker, &request, self.max_commits, MAX_HANDOFF_COMMIT_BYTES)
    }
}

fn key_in_use(member: &Workspace) -> String {
    member.storage_settings().unwrap().access_key_id
}

fn request_of(member: &Workspace) -> KeyRequest {
    member.key_request().unwrap()
}

async fn assert_reads(member: &Workspace, text: &str) {
    member.sync().await;
    assert_eq!(member.status().error, None);
    let general = channel_id(member, "general");
    let texts: Vec<String> = member
        .messages(&general, None, 50)
        .unwrap()
        .into_iter()
        .map(|message| message.text)
        .collect();
    assert!(texts.contains(&text.to_owned()), "{texts:?}");
}

/// Dave, a fourth member on `provider`, in sync with the others.
async fn dave(root: &std::path::Path, admin: &Workspace, provider: &Provider) -> Workspace {
    let dave = join_connected(
        admin,
        &root.join("dave"),
        &provider.bucket(),
        provider.connector(),
        "Dave",
    )
    .await;
    dave.sync().await;
    dave
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_away_through_a_key_change_gets_the_new_key_from_another_member() {
    let provider = Provider::new();
    let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;

    change_key(&admin, "second").await;
    sync_all(&[&admin, &bob]).await;
    provider.delete_key(FIRST_KEY);
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "after the change", None).unwrap();
    admin.sync().await;
    carol.sync().await;
    assert!(carol.status().error.is_some(), "the old key is refused");

    let state = carol
        .recover_storage_key(&Peers::new(&carol, &[&bob]))
        .await
        .unwrap();

    assert_eq!(state, KeyState::Recovered);
    assert_eq!(key_in_use(&carol), "second");
    assert_reads(&carol, "after the change").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_away_through_a_removal_gets_the_new_epoch_and_key() {
    let provider = Provider::new();
    let (root, admin, bob, carol) = admin_bob_carol(&provider).await;
    let dave = dave(root.path(), &admin, &provider).await;

    admin.remove_member(&carol.my_id()).await.unwrap();
    change_key(&admin, "second").await;
    sync_all(&[&admin, &bob]).await;
    provider.delete_key(FIRST_KEY);
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "without Carol", None).unwrap();
    admin.sync().await;

    let state = dave
        .recover_storage_key(&Peers::new(&dave, &[&bob]))
        .await
        .unwrap();

    assert_eq!(state, KeyState::Recovered);
    assert_eq!(
        dave.state().mls.epoch().unwrap(),
        admin.state().mls.epoch().unwrap()
    );
    assert_reads(&dave, "without Carol").await;
    assert!(!dave.current_members().unwrap().contains(&carol.me()));
}

/// Bob was away for the second change: what he hands over is newer than
/// Dave's key but refused too, so Dave asks someone else.
#[tokio::test(flavor = "multi_thread")]
async fn settings_that_are_newer_but_still_refused_send_the_asker_to_another_member() {
    let provider = Provider::new();
    let (root, admin, bob, carol) = admin_bob_carol(&provider).await;
    let dave = dave(root.path(), &admin, &provider).await;

    change_key(&admin, "second").await;
    sync_all(&[&admin, &bob, &carol]).await;
    change_key(&admin, "third").await;
    let general = channel_id(&admin, "general");
    admin.send_message(&general, "after the third key", None).unwrap();
    sync_all(&[&admin, &carol]).await;
    provider.delete_key(FIRST_KEY);
    provider.delete_key("second");

    let only_bob = Peers::new(&dave, &[&bob]);
    assert_eq!(
        dave.recover_storage_key(&only_bob).await.unwrap(),
        KeyState::Refused
    );
    assert_eq!(
        key_in_use(&dave),
        "second",
        "Bob's settings were newer, so they were taken"
    );

    // Carol comes online.
    dave.receive_heartbeat(&carol.heartbeat(PresenceState::Online).unwrap())
        .unwrap();
    let both = Peers::new(&dave, &[&bob, &carol]);
    assert_eq!(
        dave.recover_storage_key(&both).await.unwrap(),
        KeyState::Recovered
    );
    assert_eq!(key_in_use(&dave), "third");
    assert_eq!(both.times_asked(&bob), 0, "Bob already answered this round");
    assert_reads(&dave, "after the third key").await;
    assert!(
        matches!(
            dave.key_answer_for(&bob.me(), &request_of(&bob)).unwrap(),
            KeyAnswer::Settings { .. }
        ),
        "once the op itself arrived, Dave can pass the settings on"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn one_member_saying_too_far_behind_is_not_believed_over_another() {
    let provider = Provider::new();
    let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;
    change_key(&admin, "second").await;
    provider.delete_key(FIRST_KEY);
    let peers = Peers {
        lies: vec![
            (bob.me(), KeyAnswer::TooFarBehind),
            (admin.me(), KeyAnswer::NothingNewer),
        ],
        ..Peers::new(&carol, &[])
    };

    assert_eq!(
        carol.recover_storage_key(&peers).await.unwrap(),
        KeyState::Refused
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn members_out_of_reach_are_tried_once_a_round_then_one_a_pass() {
    let provider = Provider::new();
    let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;
    change_key(&admin, "second").await;
    provider.delete_key(FIRST_KEY);
    let nobody = Peers::new(&carol, &[]);

    assert_eq!(
        carol.recover_storage_key(&nobody).await.unwrap(),
        KeyState::Refused
    );
    assert_eq!((nobody.times_asked(&admin), nobody.times_asked(&bob)), (1, 1));
    carol.recover_storage_key(&nobody).await.unwrap();
    assert_eq!(nobody.asked.lock().unwrap().len(), 3, "one retry a pass");
}

#[tokio::test(flavor = "multi_thread")]
async fn more_commit_bytes_than_an_answer_carries_count_as_too_far_behind() {
    let provider = Provider::new();
    let (root, admin, bob, carol) = admin_bob_carol(&provider).await;
    let dave = dave(root.path(), &admin, &provider).await;
    let request = request_of(&dave);
    admin.remove_member(&carol.my_id()).await.unwrap();
    change_key(&admin, "second").await;
    sync_all(&[&admin, &bob]).await;

    assert_eq!(
        bob.key_answer_within(&dave.me(), &request, MAX_HANDOFF_COMMITS, 1)
            .unwrap(),
        KeyAnswer::TooFarBehind
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_removed_while_away_is_handed_nothing() {
    let provider = Provider::new();
    let (root, admin, bob, _carol) = admin_bob_carol(&provider).await;
    let dave = dave(root.path(), &admin, &provider).await;
    let asked_before_bob_heard = request_of(&dave);

    admin.remove_member(&dave.my_id()).await.unwrap();
    change_key(&admin, "second").await;
    provider.delete_key(FIRST_KEY);
    // Bob learns of the removal only now, after the request was made.
    bob.sync().await;

    assert!(bob.key_answer_for(&dave.me(), &asked_before_bob_heard).is_err());
    assert!(admin.key_answer_for(&dave.me(), &request_of(&dave)).is_err());
    let state = dave
        .recover_storage_key(&Peers::new(&dave, &[&admin, &bob]))
        .await
        .unwrap();
    assert_eq!(state, KeyState::Refused);
    assert_eq!(key_in_use(&dave), FIRST_KEY);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_too_far_behind_is_told_so_instead_of_handed_everything() {
    let provider = Provider::new();
    let (root, admin, bob, carol) = admin_bob_carol(&provider).await;
    let dave = dave(root.path(), &admin, &provider).await;
    let erin = join_connected(
        &admin,
        &root.path().join("erin"),
        &provider.bucket(),
        provider.connector(),
        "Erin",
    )
    .await;
    sync_all(&[&admin, &bob, &carol, &dave, &erin]).await;

    admin.remove_member(&carol.my_id()).await.unwrap();
    admin.remove_member(&erin.my_id()).await.unwrap();
    change_key(&admin, "second").await;
    sync_all(&[&admin, &bob]).await;
    provider.delete_key(FIRST_KEY);

    let one_commit_at_most = Peers {
        max_commits: 1,
        ..Peers::new(&dave, &[&bob])
    };
    let state = dave.recover_storage_key(&one_commit_at_most).await.unwrap();
    assert_eq!(state, KeyState::TooFarBehind);
    assert_eq!(key_in_use(&dave), FIRST_KEY);

    let request = request_of(&dave);
    assert!(matches!(
        bob.key_answer_for(&dave.me(), &request).unwrap(),
        KeyAnswer::Settings { commits, .. } if commits.len() == 2
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_whose_key_works_asks_nobody() {
    let provider = Provider::new();
    let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;
    let peers = Peers::new(&carol, &[&admin, &bob]);

    assert_eq!(
        carol.recover_storage_key(&peers).await.unwrap(),
        KeyState::Working
    );
    assert!(peers.asked.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn members_with_nothing_newer_say_so() {
    let provider = Provider::new();
    let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;
    change_key(&admin, "second").await;
    sync_all(&[&admin, &bob, &carol]).await;

    assert_eq!(
        bob.key_answer_for(&carol.me(), &request_of(&carol)).unwrap(),
        KeyAnswer::NothingNewer
    );
}

/// Bob applied the new settings with a version that didn't record which op
/// set them; he still finds that op to hand it over.
#[tokio::test(flavor = "multi_thread")]
async fn settings_applied_before_the_record_existed_are_still_handed_over() {
    let provider = Provider::new();
    let (_root, admin, bob, carol) = admin_bob_carol(&provider).await;
    change_key(&admin, "second").await;
    sync_all(&[&admin, &bob]).await;
    provider.delete_key(FIRST_KEY);
    bob.state()
        .store
        .delete_meta(crate::config::SETTINGS_OP_META)
        .unwrap();

    let state = carol
        .recover_storage_key(&Peers::new(&carol, &[&bob]))
        .await
        .unwrap();

    assert_eq!(state, KeyState::Recovered);
    assert_eq!(key_in_use(&carol), "second");
    assert_eq!(
        crate::config::settings_op_seq(&carol.state().store).unwrap(),
        Some(crate::config::NO_SETTINGS_OP),
        "handed over, so no search until the op itself arrives"
    );
    let recorded = crate::config::settings_op_seq(&bob.state().store).unwrap();
    assert!(recorded.is_some_and(|seq| seq > 0), "found once, then recorded");
}

/// Settings that came with an invite have no op to hand over, and are
/// recorded so: nobody searches the admin's log for one.
#[tokio::test(flavor = "multi_thread")]
async fn a_joiner_records_that_its_settings_came_with_the_invite() {
    let provider = Provider::new();
    let (root, admin, _bob, _carol) = admin_bob_carol(&provider).await;
    let dave = dave(root.path(), &admin, &provider).await;

    assert_eq!(
        crate::config::settings_op_seq(&dave.state().store).unwrap(),
        Some(crate::config::NO_SETTINGS_OP)
    );
}
