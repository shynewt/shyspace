//! Who is on a page and where their cursor is: small signed messages that
//! travel live between the members on the page and are never stored. A
//! beacon says "I'm on this page" (or "I left it"); a cursor update says
//! where my selection is, as Yjs relative positions, so it stays in place as
//! the text around it changes. Both are signed by the member's identity key
//! for this workspace and page and carry their send time; anything badly
//! signed, stale, out of order, from someone who isn't a current member, or
//! beyond a member's rate is refused.

use std::collections::HashMap;

use anyhow::{Context, Result, ensure};
use iroh::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};

use crate::{
    ops::{EntityId, MAX_ID_CHARS},
    private::ids::is_private_id,
};

const BEACON_DOMAIN: &str = "shyspace page beacon v1";
const CURSOR_DOMAIN: &str = "shyspace page cursor v1";
/// A message sent further from now than this, by the sender's clock, is
/// refused, so a recorded one can't be replayed later.
const MAX_CLOCK_SKEW_MS: i64 = 30 * 1000;
/// Relative positions are a client id and a clock or two; anything longer is
/// not one.
pub const MAX_POSITION_BYTES: usize = 64;
/// Senders send about 10 cursor updates a second; twice that from one member
/// within a second is flooding, and the rest of that second is dropped.
const MAX_CURSOR_UPDATES_PER_SECOND: u32 = 20;
/// Beacons go out every 15 seconds and on opening or leaving a page.
const MIN_BEACON_GAP_MS: i64 = 1000;
/// Someone not heard on a page for this long has left it.
pub const VIEWER_TIMEOUT_MS: i64 = 40 * 1000;
/// Beacons are repeated this often while a page stays open.
pub const BEACON_EVERY_MS: i64 = 15 * 1000;

/// "I'm on this page", or with `is_open` false, "I left it".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageBeacon {
    pub member: PublicKey,
    pub page: EntityId,
    pub is_open: bool,
    pub sent_ms: i64,
    pub signature: Vec<u8>,
}

/// A selection in a page: where it starts and where the caret is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorSelection {
    pub anchor: Vec<u8>,
    pub head: Vec<u8>,
}

/// Where a member's cursor is on a page; none when they aren't in the text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorUpdate {
    pub member: PublicKey,
    pub page: EntityId,
    pub selection: Option<CursorSelection>,
    pub sent_ms: i64,
    pub signature: Vec<u8>,
}

impl PageBeacon {
    pub fn signed(
        identity: &SecretKey,
        workspace_id: &str,
        page: &str,
        is_open: bool,
        sent_ms: i64,
    ) -> Result<Self> {
        let member = identity.public();
        let bytes = beacon_bytes(workspace_id, &member, page, is_open, sent_ms)?;
        Ok(Self {
            member,
            page: page.to_owned(),
            is_open,
            sent_ms,
            signature: identity.sign(&bytes).to_bytes().to_vec(),
        })
    }

    fn check(&self, workspace_id: &str, is_member: impl Fn(&PublicKey) -> bool, now_ms: i64) -> Result<()> {
        check_page(&self.page)?;
        check_fresh(self.sent_ms, now_ms)?;
        ensure!(
            is_member(&self.member),
            "page beacon from someone who isn't a member"
        );
        let bytes = beacon_bytes(workspace_id, &self.member, &self.page, self.is_open, self.sent_ms)?;
        verify(&self.member, &bytes, &self.signature).context("bad page beacon signature")
    }
}

impl CursorUpdate {
    pub fn signed(
        identity: &SecretKey,
        workspace_id: &str,
        page: &str,
        selection: Option<CursorSelection>,
        sent_ms: i64,
    ) -> Result<Self> {
        let member = identity.public();
        let bytes = cursor_bytes(workspace_id, &member, page, selection.as_ref(), sent_ms)?;
        Ok(Self {
            member,
            page: page.to_owned(),
            selection,
            sent_ms,
            signature: identity.sign(&bytes).to_bytes().to_vec(),
        })
    }

    fn check(&self, workspace_id: &str, is_member: impl Fn(&PublicKey) -> bool, now_ms: i64) -> Result<()> {
        check_page(&self.page)?;
        if let Some(selection) = &self.selection {
            ensure!(
                selection.anchor.len() <= MAX_POSITION_BYTES && selection.head.len() <= MAX_POSITION_BYTES,
                "cursor positions too long"
            );
        }
        check_fresh(self.sent_ms, now_ms)?;
        ensure!(
            is_member(&self.member),
            "cursor update from someone who isn't a member"
        );
        let bytes = cursor_bytes(
            workspace_id,
            &self.member,
            &self.page,
            self.selection.as_ref(),
            self.sent_ms,
        )?;
        verify(&self.member, &bytes, &self.signature).context("bad cursor update signature")
    }
}

fn beacon_bytes(
    workspace_id: &str,
    member: &PublicKey,
    page: &str,
    is_open: bool,
    sent_ms: i64,
) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(
        BEACON_DOMAIN,
        workspace_id,
        member.as_bytes(),
        page,
        is_open,
        sent_ms,
    ))?)
}

fn cursor_bytes(
    workspace_id: &str,
    member: &PublicKey,
    page: &str,
    selection: Option<&CursorSelection>,
    sent_ms: i64,
) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(
        CURSOR_DOMAIN,
        workspace_id,
        member.as_bytes(),
        page,
        selection,
        sent_ms,
    ))?)
}

/// Pages are workspace-wide; a private id names something only some members may know of.
fn check_page(page: &str) -> Result<()> {
    ensure!(
        !page.is_empty() && page.len() <= MAX_ID_CHARS,
        "page id out of range"
    );
    ensure!(!is_private_id(page), "not a page id");
    Ok(())
}

fn check_fresh(sent_ms: i64, now_ms: i64) -> Result<()> {
    ensure!(
        sent_ms.abs_diff(now_ms) <= MAX_CLOCK_SKEW_MS.unsigned_abs(),
        "sent too long ago or from the future"
    );
    Ok(())
}

fn verify(member: &PublicKey, bytes: &[u8], signature: &[u8]) -> Result<()> {
    let signature: [u8; 64] = signature.try_into().context("malformed signature")?;
    member.verify(bytes, &Signature::from_bytes(&signature))?;
    Ok(())
}

/// Who is on which page, as their beacons say, by our clock.
#[derive(Default)]
pub struct PageViewers {
    /// Page → member → (their last beacon's send time, when we got it).
    pages: HashMap<EntityId, HashMap<PublicKey, (i64, i64)>>,
}

impl PageViewers {
    /// Takes a checked beacon; returns whether who's on its page changed.
    pub fn accept(
        &mut self,
        beacon: &PageBeacon,
        workspace_id: &str,
        is_member: impl Fn(&PublicKey) -> bool,
        now_ms: i64,
    ) -> Result<bool> {
        beacon.check(workspace_id, is_member, now_ms)?;
        let was_on = self.viewers(&beacon.page, now_ms).contains(&beacon.member);
        let viewers = self.pages.entry(beacon.page.clone()).or_default();
        if let Some(&(sent_ms, received_ms)) = viewers.get(&beacon.member) {
            ensure!(
                beacon.sent_ms > sent_ms,
                "page beacon older than one already taken"
            );
            ensure!(
                now_ms - received_ms >= MIN_BEACON_GAP_MS,
                "page beacons too close together"
            );
        }
        if beacon.is_open {
            viewers.insert(beacon.member, (beacon.sent_ms, now_ms));
        } else {
            viewers.remove(&beacon.member);
        }
        Ok(was_on != beacon.is_open)
    }

    /// The members on `page` now, in no particular order.
    pub fn viewers(&self, page: &str, now_ms: i64) -> Vec<PublicKey> {
        self.pages.get(page).map_or_else(Vec::new, |viewers| {
            viewers
                .iter()
                .filter(|(_, (_, received_ms))| now_ms - received_ms < VIEWER_TIMEOUT_MS)
                .map(|(member, _)| *member)
                .collect()
        })
    }

    /// A removed member is on no page.
    pub fn forget_member(&mut self, member: &PublicKey) {
        for viewers in self.pages.values_mut() {
            viewers.remove(member);
        }
    }
}

/// A member's cursor on a page, as last heard.
#[derive(Debug, Clone, PartialEq)]
pub struct HeardCursor {
    pub selection: Option<CursorSelection>,
    pub sent_ms: i64,
    pub received_ms: i64,
}

/// The cursors heard on pages, by our clock, with each member's rate.
#[derive(Default)]
pub struct Cursors {
    heard: HashMap<(EntityId, PublicKey), HeardCursor>,
    /// Member → (the second we're counting, updates taken in it).
    rates: HashMap<PublicKey, (i64, u32)>,
}

impl Cursors {
    /// Takes a checked update; returns whether the cursor it describes moved.
    pub fn accept(
        &mut self,
        update: &CursorUpdate,
        workspace_id: &str,
        is_member: impl Fn(&PublicKey) -> bool,
        now_ms: i64,
    ) -> Result<bool> {
        update.check(workspace_id, is_member, now_ms)?;
        // Before counting: a replay of someone's own updates mustn't use up their rate.
        let key = (update.page.clone(), update.member);
        if let Some(heard) = self.heard.get(&key) {
            ensure!(
                update.sent_ms > heard.sent_ms,
                "cursor update older than one already taken"
            );
        }
        let second = now_ms.div_euclid(1000);
        let (counted_second, count) = self.rates.entry(update.member).or_insert((second, 0));
        if *counted_second != second {
            *counted_second = second;
            *count = 0;
        }
        ensure!(*count < MAX_CURSOR_UPDATES_PER_SECOND, "cursor updates too fast");
        *count += 1;
        let moved = self
            .heard
            .get(&key)
            .is_none_or(|heard| heard.selection != update.selection);
        self.heard.insert(
            key,
            HeardCursor {
                selection: update.selection.clone(),
                sent_ms: update.sent_ms,
                received_ms: now_ms,
            },
        );
        Ok(moved)
    }

    /// The cursors of `viewers` on `page`, as last heard.
    pub fn on_page(&self, page: &str, viewers: &[PublicKey]) -> Vec<(PublicKey, HeardCursor)> {
        viewers
            .iter()
            .filter_map(|member| {
                self.heard
                    .get(&(page.to_owned(), *member))
                    .map(|heard| (*member, heard.clone()))
            })
            .collect()
    }

    /// A removed member's cursors go everywhere at once.
    pub fn forget_member(&mut self, member: &PublicKey) {
        self.heard.retain(|(_, holder), _| holder != member);
        self.rates.remove(member);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKSPACE: &str = "workspace-id";
    const PAGE: &str = "page-1";
    const NOW: i64 = 1_800_000_000_000;

    fn key(seed: u8) -> SecretKey {
        SecretKey::from_bytes(&[seed; 32])
    }

    fn selection(at: u8) -> Option<CursorSelection> {
        Some(CursorSelection {
            anchor: vec![at; 8],
            head: vec![at; 8],
        })
    }

    fn update(identity: &SecretKey, at: u8, sent_ms: i64) -> CursorUpdate {
        CursorUpdate::signed(identity, WORKSPACE, PAGE, selection(at), sent_ms).unwrap()
    }

    fn anyone(_: &PublicKey) -> bool {
        true
    }

    #[test]
    fn a_members_cursor_is_heard_where_it_moves() {
        let ada = key(1);
        let mut cursors = Cursors::default();
        assert!(
            cursors
                .accept(&update(&ada, 1, NOW), WORKSPACE, anyone, NOW)
                .unwrap()
        );
        assert!(
            !cursors
                .accept(&update(&ada, 1, NOW + 100), WORKSPACE, anyone, NOW + 100)
                .unwrap()
        );
        assert!(
            cursors
                .accept(&update(&ada, 2, NOW + 200), WORKSPACE, anyone, NOW + 200)
                .unwrap()
        );
        let heard = cursors.on_page(PAGE, &[ada.public()]);
        assert_eq!(heard.len(), 1);
        assert_eq!(heard[0].1.selection, selection(2));
    }

    #[test]
    fn a_forged_cursor_update_is_refused() {
        let (ada, mallory) = (key(1), key(2));
        let mut forged = update(&mallory, 1, NOW);
        forged.member = ada.public();
        let mut cursors = Cursors::default();
        assert!(cursors.accept(&forged, WORKSPACE, anyone, NOW).is_err());

        let mut moved = update(&ada, 1, NOW);
        moved.selection = selection(9);
        assert!(
            cursors.accept(&moved, WORKSPACE, anyone, NOW).is_err(),
            "a changed position breaks the signature"
        );
        let mut elsewhere = update(&ada, 1, NOW);
        elsewhere.page = "page-2".into();
        assert!(
            cursors.accept(&elsewhere, WORKSPACE, anyone, NOW).is_err(),
            "it's bound to its page"
        );
        assert!(
            cursors
                .accept(&update(&ada, 1, NOW), "another-workspace", anyone, NOW)
                .is_err(),
            "and to its workspace"
        );
        assert!(cursors.on_page(PAGE, &[ada.public()]).is_empty());
    }

    #[test]
    fn stale_and_replayed_cursor_updates_are_refused() {
        let ada = key(1);
        let mut cursors = Cursors::default();
        let old = update(&ada, 1, NOW - MAX_CLOCK_SKEW_MS - 1);
        assert!(cursors.accept(&old, WORKSPACE, anyone, NOW).is_err());
        let ahead = update(&ada, 1, NOW + MAX_CLOCK_SKEW_MS + 1);
        assert!(cursors.accept(&ahead, WORKSPACE, anyone, NOW).is_err());

        let first = update(&ada, 1, NOW);
        let second = update(&ada, 2, NOW + 100);
        cursors.accept(&second, WORKSPACE, anyone, NOW + 100).unwrap();
        assert!(
            cursors.accept(&first, WORKSPACE, anyone, NOW + 200).is_err(),
            "out of order"
        );
        assert!(
            cursors.accept(&second, WORKSPACE, anyone, NOW + 300).is_err(),
            "replayed"
        );
        assert_eq!(
            cursors.on_page(PAGE, &[ada.public()])[0].1.selection,
            selection(2)
        );
    }

    #[test]
    fn a_member_flooding_cursor_updates_is_cut_off_for_the_second() {
        let ada = key(1);
        let mut cursors = Cursors::default();
        let start = NOW - NOW.rem_euclid(1000);
        let taken = (0..100)
            .filter(|index| {
                let sent = start + index;
                cursors
                    .accept(&update(&ada, (*index % 7) as u8, sent), WORKSPACE, anyone, sent)
                    .is_ok()
            })
            .count();
        assert_eq!(taken, MAX_CURSOR_UPDATES_PER_SECOND as usize);
        let next_second = start + 1000;
        assert!(
            cursors
                .accept(&update(&ada, 3, next_second), WORKSPACE, anyone, next_second)
                .is_ok()
        );
    }

    #[test]
    fn replaying_a_members_updates_doesnt_use_up_their_rate() {
        let ada = key(1);
        let mut cursors = Cursors::default();
        let start = NOW - NOW.rem_euclid(1000);
        let taken = update(&ada, 1, start);
        cursors.accept(&taken, WORKSPACE, anyone, start).unwrap();
        for replay in 0..100 {
            assert!(cursors.accept(&taken, WORKSPACE, anyone, start + replay).is_err());
        }
        assert!(
            cursors
                .accept(&update(&ada, 2, start + 200), WORKSPACE, anyone, start + 200)
                .is_ok()
        );
    }

    #[test]
    fn oversized_positions_and_private_ids_are_refused() {
        let ada = key(1);
        let mut cursors = Cursors::default();
        let long = Some(CursorSelection {
            anchor: vec![0; MAX_POSITION_BYTES + 1],
            head: vec![0; 8],
        });
        let oversized = CursorUpdate::signed(&ada, WORKSPACE, PAGE, long, NOW).unwrap();
        assert!(cursors.accept(&oversized, WORKSPACE, anyone, NOW).is_err());
        let private = CursorUpdate::signed(&ada, WORKSPACE, "p:abcd/ef01", selection(1), NOW).unwrap();
        assert!(cursors.accept(&private, WORKSPACE, anyone, NOW).is_err());
    }

    #[test]
    fn non_members_and_removed_members_have_no_cursor_and_are_on_no_page() {
        let (ada, outsider) = (key(1), key(2));
        let members = |member: &PublicKey| *member == ada.public();
        let mut cursors = Cursors::default();
        let mut viewers = PageViewers::default();
        assert!(
            cursors
                .accept(&update(&outsider, 1, NOW), WORKSPACE, members, NOW)
                .is_err()
        );
        let outsider_beacon = PageBeacon::signed(&outsider, WORKSPACE, PAGE, true, NOW).unwrap();
        assert!(viewers.accept(&outsider_beacon, WORKSPACE, members, NOW).is_err());

        let beacon = PageBeacon::signed(&ada, WORKSPACE, PAGE, true, NOW).unwrap();
        viewers.accept(&beacon, WORKSPACE, members, NOW).unwrap();
        cursors
            .accept(&update(&ada, 1, NOW), WORKSPACE, members, NOW)
            .unwrap();
        cursors.forget_member(&ada.public());
        viewers.forget_member(&ada.public());
        assert!(viewers.viewers(PAGE, NOW).is_empty());
        assert!(cursors.on_page(PAGE, &[ada.public()]).is_empty());

        let removed = |_: &PublicKey| false;
        let later = update(&ada, 2, NOW + 1000);
        assert!(
            cursors.accept(&later, WORKSPACE, removed, NOW + 1000).is_err(),
            "removed is no longer a member"
        );
    }

    #[test]
    fn viewers_come_and_go_by_their_beacons_and_time_out() {
        let ada = key(1);
        let mut viewers = PageViewers::default();
        let open = PageBeacon::signed(&ada, WORKSPACE, PAGE, true, NOW).unwrap();
        assert!(viewers.accept(&open, WORKSPACE, anyone, NOW).unwrap());
        assert_eq!(viewers.viewers(PAGE, NOW), [ada.public()]);
        assert!(
            viewers.viewers(PAGE, NOW + VIEWER_TIMEOUT_MS).is_empty(),
            "silent too long"
        );

        let again = PageBeacon::signed(&ada, WORKSPACE, PAGE, true, NOW + BEACON_EVERY_MS).unwrap();
        viewers
            .accept(&again, WORKSPACE, anyone, NOW + BEACON_EVERY_MS)
            .unwrap();
        let left = PageBeacon::signed(&ada, WORKSPACE, PAGE, false, NOW + BEACON_EVERY_MS + 2000).unwrap();
        assert!(
            viewers
                .accept(&left, WORKSPACE, anyone, NOW + BEACON_EVERY_MS + 2000)
                .unwrap()
        );
        assert!(viewers.viewers(PAGE, NOW + BEACON_EVERY_MS + 2000).is_empty());
    }

    #[test]
    fn beacons_too_close_together_or_replayed_are_refused() {
        let ada = key(1);
        let mut viewers = PageViewers::default();
        let open = PageBeacon::signed(&ada, WORKSPACE, PAGE, true, NOW).unwrap();
        viewers.accept(&open, WORKSPACE, anyone, NOW).unwrap();
        let soon = PageBeacon::signed(&ada, WORKSPACE, PAGE, false, NOW + 10).unwrap();
        assert!(viewers.accept(&soon, WORKSPACE, anyone, NOW + 10).is_err());
        assert!(
            viewers.accept(&open, WORKSPACE, anyone, NOW + 5000).is_err(),
            "replayed"
        );
        let mut forged = PageBeacon::signed(&key(2), WORKSPACE, PAGE, false, NOW + 5000).unwrap();
        forged.member = ada.public();
        assert!(viewers.accept(&forged, WORKSPACE, anyone, NOW + 5000).is_err());
        assert_eq!(viewers.viewers(PAGE, NOW + 5000), [ada.public()]);
    }
}
