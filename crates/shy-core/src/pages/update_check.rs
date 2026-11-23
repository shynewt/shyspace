//! Checks a page update from another member before yrs decodes it.
//!
//! yrs trusts what an update says. It decodes nested values by recursion (a
//! 16 KB update overflows the stack), reserves memory from claimed counts,
//! adds clocks without checking for overflow, and assumes nobody sends other
//! content for a position it already has or leaves holes; updates that break
//! that last assumption make it panic, loop forever or crash. A page is one
//! Yjs text, and an honest editor only ever appends its own new pieces: text
//! or deleted text on that text, numbered right after its previous ones, plus
//! a sorted list of what it deleted. This walks the update (Yjs's v1
//! encoding) and refuses anything else, before yrs sees it.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, bail, ensure};
use yrs::encoding::read::{Cursor, Read};

use super::CONTENT;

const GC: u8 = 0;
const SKIP: u8 = 10;
const HAS_ORIGIN: u8 = 0b1000_0000;
const HAS_RIGHT_ORIGIN: u8 = 0b0100_0000;
const HAS_PARENT_SUB: u8 = 0b0010_0000;
const CONTENT_KIND: u8 = 0b0001_1111;
const DELETED: u8 = 1;
const STRING: u8 = 4;
const PARENT_IS_NAMED: u64 = 1;
/// yrs keeps client ids in 53 bits (JavaScript's safe integers).
pub(super) const MAX_CLIENT_ID: u64 = (1 << 53) - 1;
/// yrs panics on pieces past clock 2^31, though it stores clocks in 32
/// bits. A client's clock grows by one per character it ever wrote, so
/// honest editors stay far below this.
const MAX_CLOCK: u32 = 1 << 30;
/// yrs's cost per deleted range grows with the ranges before it: 100,000
/// take about a third of a second, a 4 MB update's worth half a minute.
/// Even a large find-and-replace deletes far fewer places at once.
const MAX_DELETED_RANGES: u64 = 100_000;

/// Where each Yjs client's pieces end on a page, counting pieces yrs keeps
/// waiting for others they build on. An update must continue from there.
#[derive(Default)]
pub(super) struct ClientEnds(HashMap<u64, u32>);

impl ClientEnds {
    /// Reads them from a page state this app encoded.
    pub(super) fn of_state(state: &[u8]) -> Result<Self> {
        let mut reader = UpdateReader::new(state);
        let mut ends = ClientEnds::default();
        for _ in 0..reader.count()? {
            let blocks = reader.count()?;
            let client = reader.client()?;
            let mut clock = reader.number()?;
            for _ in 0..blocks {
                let (Piece::Gap(length) | Piece::Item(length)) = reader.piece()?;
                clock = clock.checked_add(length).context("clock past what yrs holds")?;
            }
            ends.record(client, clock);
        }
        Ok(ends)
    }

    pub(super) fn extend(&mut self, ends: Vec<(u64, u32)>) {
        for (client, end) in ends {
            self.record(client, end);
        }
    }

    fn record(&mut self, client: u64, end: u32) {
        let known = self.0.entry(client).or_default();
        *known = (*known).max(end);
    }

    fn end_of(&self, client: u64) -> u32 {
        self.0.get(&client).copied().unwrap_or(0)
    }
}

/// Fails, saying why, unless `update` only appends text or deleted text to
/// the page's text, each client's pieces right after its last ones in
/// `ends`, with a well-formed list of deletions, and adds pieces only for
/// Yjs client `client`. Its pieces may still name any client's as
/// neighbours, and it may delete anyone's text. Returns the clients' new
/// ends, to record once the update applies.
pub(super) fn check_update(update: &[u8], ends: &ClientEnds, client: u64) -> Result<Vec<(u64, u32)>> {
    let mut reader = UpdateReader::new(update);
    let new_ends = reader.blocks(ends).context("page update blocks")?;
    ensure!(
        new_ends.iter().all(|&(written, _)| written == client),
        "page update adds pieces for another editor's client id"
    );
    reader.delete_set().context("page update deletions")?;
    ensure!(reader.remaining() == 0, "page update has bytes after its end");
    Ok(new_ends)
}

enum Piece {
    /// A GC or skip block: a run of clocks without content.
    Gap(u32),
    Item(u32),
}

struct UpdateReader<'a> {
    cursor: Cursor<'a>,
}

impl<'a> UpdateReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        UpdateReader {
            cursor: Cursor::new(bytes),
        }
    }

    fn blocks(&mut self, ends: &ClientEnds) -> Result<Vec<(u64, u32)>> {
        let clients = self.count()?;
        let mut new_ends = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..clients {
            let blocks = self.count()?;
            ensure!(blocks > 0, "a client with no pieces");
            let client = self.client()?;
            ensure!(seen.insert(client), "a client listed twice");
            let mut clock = self.number()?;
            ensure!(
                clock == ends.end_of(client),
                "pieces that don't continue their client's text"
            );
            for _ in 0..blocks {
                let Piece::Item(length) = self.piece()? else {
                    bail!("a gap in a client's pieces");
                };
                clock = clock.checked_add(length).context("clock past what yrs holds")?;
            }
            ensure!(clock < MAX_CLOCK, "pieces past clock {MAX_CLOCK}");
            new_ends.push((client, clock));
        }
        Ok(new_ends)
    }

    fn piece(&mut self) -> Result<Piece> {
        let info = self.cursor.read_u8()?;
        if info == GC || info == SKIP {
            let length = self.number()?;
            ensure!(length > 0, "empty gap");
            return Ok(Piece::Gap(length));
        }
        if info & HAS_ORIGIN != 0 {
            self.id()?;
        }
        if info & HAS_RIGHT_ORIGIN != 0 {
            self.id()?;
        }
        if info & (HAS_ORIGIN | HAS_RIGHT_ORIGIN) == 0 {
            // Without origins, an item names its parent: here it must be the
            // page's text itself, not another root, an item or a map key.
            ensure!(self.var()? == PARENT_IS_NAMED, "item inside another item");
            ensure!(
                self.cursor.read_string()? == CONTENT,
                "item outside the page text"
            );
            ensure!(info & HAS_PARENT_SUB == 0, "item under a key");
        }
        let length = match info & CONTENT_KIND {
            DELETED => self.number()?,
            STRING => {
                let text = self.cursor.read_string()?;
                u32::try_from(text.encode_utf16().count()).context("text too long")?
            }
            kind => bail!("content kind {kind} is not page text"),
        };
        ensure!(length > 0, "empty item");
        Ok(Piece::Item(length))
    }

    fn delete_set(&mut self) -> Result<()> {
        let clients = self.count()?;
        let mut seen = HashSet::new();
        let mut ranges_left = MAX_DELETED_RANGES;
        for _ in 0..clients {
            ensure!(seen.insert(self.client()?), "a client listed twice");
            let ranges = self.count()?;
            ranges_left = ranges_left
                .checked_sub(ranges)
                .context("more deleted ranges than one update may hold")?;
            // Yjs sorts and joins ranges; yrs miscounts the text's length
            // when one transaction deletes two touching ranges that split a
            // character written as two UTF-16 units.
            let mut previous_end = None;
            for _ in 0..ranges {
                let start = self.number()?;
                let length = self.number()?;
                ensure!(length > 0, "empty deleted range");
                ensure!(
                    previous_end.is_none_or(|end| start > end),
                    "deleted ranges out of order, overlapping or touching"
                );
                previous_end = Some(
                    start
                        .checked_add(length)
                        .context("deleted range past what yrs holds")?,
                );
            }
        }
        Ok(())
    }

    fn id(&mut self) -> Result<()> {
        self.client()?;
        self.number()?;
        Ok(())
    }

    fn client(&mut self) -> Result<u64> {
        let client = self.var()?;
        ensure!(client <= MAX_CLIENT_ID, "client id over 53 bits");
        Ok(client)
    }

    /// A count of things that follow: each takes at least a byte, so a
    /// count larger than the bytes left is a lie meant to reserve memory.
    fn count(&mut self) -> Result<u64> {
        let count = self.var()?;
        ensure!(count <= self.remaining(), "count {count} larger than the update");
        Ok(count)
    }

    /// A clock or length, which yrs keeps in 32 bits.
    fn number(&mut self) -> Result<u32> {
        let number = self.var()?;
        u32::try_from(number).context("number past what yrs holds")
    }

    fn var(&mut self) -> Result<u64> {
        Ok(self.cursor.read_var::<u64>()?)
    }

    fn remaining(&self) -> u64 {
        (self.cursor.buf.len() - self.cursor.next) as u64
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::pages::{Edit, PageMerge, edit, merge, read};

    fn from_hex(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
            .collect()
    }

    fn var(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push((value as u8 & 0x7f) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    fn text_item(text: &str) -> Vec<u8> {
        let mut item = vec![4, 1, 7];
        item.extend_from_slice(b"content");
        var(&mut item, text.len() as u64);
        item.extend_from_slice(text.as_bytes());
        item
    }

    /// An update with one client's pieces and no deletions.
    fn pieces(client: u64, clock: u64, items: &[Vec<u8>]) -> Vec<u8> {
        let mut update = vec![1];
        var(&mut update, items.len() as u64);
        var(&mut update, client);
        var(&mut update, clock);
        for item in items {
            update.extend_from_slice(item);
        }
        update.push(0);
        update
    }

    /// An update with no pieces, only deleted ranges for client 1.
    fn deletions(ranges: &[(u64, u64)]) -> Vec<u8> {
        let mut update = vec![0, 1, 1];
        var(&mut update, ranges.len() as u64);
        for (start, length) in ranges {
            var(&mut update, *start);
            var(&mut update, *length);
        }
        update
    }

    /// The client an update adds pieces for first, as if its author were
    /// that client, so the checks other than the client's own get tested.
    fn first_client(update: &[u8]) -> u64 {
        let mut cursor = Cursor::new(update);
        let mut first = || -> Result<u64> {
            if cursor.read_var::<u64>()? == 0 {
                return Ok(0);
            }
            cursor.read_var::<u64>()?;
            Ok(cursor.read_var::<u64>()?)
        };
        first().unwrap_or(0)
    }

    /// `update` split into one update per client, each with its deletions,
    /// as its authors would each have to send it now. One the checker can't
    /// even read stays whole: it is refused for that.
    fn one_update_per_client(update: &[u8]) -> Vec<Vec<u8>> {
        let split = || -> Result<Vec<Vec<u8>>> {
            let mut reader = UpdateReader::new(update);
            let clients = reader.count()?;
            if clients == 0 {
                return Ok(vec![update.to_vec()]);
            }
            let mut sections = Vec::new();
            for _ in 0..clients {
                let start = reader.cursor.next;
                let blocks = reader.count()?;
                reader.client()?;
                reader.number()?;
                for _ in 0..blocks {
                    reader.piece()?;
                }
                sections.push(&update[start..reader.cursor.next]);
            }
            let deletions = &update[reader.cursor.next..];
            Ok(sections
                .into_iter()
                .map(|section| [&[1][..], section, deletions].concat())
                .collect())
        };
        split().unwrap_or_else(|_| vec![update.to_vec()])
    }

    fn check_on_empty_page(update: &[u8]) -> Result<Vec<(u64, u32)>> {
        check_update(update, &ClientEnds::default(), first_client(update))
    }

    fn merge_as_its_client(state: Option<&[u8]>, update: &[u8]) -> Result<Vec<u8>> {
        merge(state, update, first_client(update))
    }

    fn merge_in_order(updates: &[Vec<u8>]) -> Result<Vec<u8>> {
        let mut state = None;
        for update in updates {
            state = Some(merge_as_its_client(state.as_deref(), update)?);
        }
        Ok(state.unwrap_or_default())
    }

    /// Made by the desktop editor's Yjs (13.6), in order: two clients typing,
    /// deleting and adding an emoji.
    fn edits_from_yjs() -> Vec<Vec<u8>> {
        [
            "0101fef9b3b40800040107636f6e74656e741a2320506c616e0a0a5368697020697420f09f9a8020736f6f6e2e00",
            "0001fef9b3b408010204",
            "0101fef9b3b40818c4fef9b3b40805fef9b3b4080607526f61646d617000",
            "0101bd96a08f080084fef9b3b408170a0a2d207374657020c3a901fef9b3b408010001",
        ]
        .iter()
        .map(|hex| from_hex(hex))
        .collect()
    }

    /// Made by Yjs 13.6.33, editors setting `doc.clientID` as the desktop
    /// editor does: client 777 starts the page, then client 4242 types a
    /// batch merged with `Y.mergeUpdates`, deletes text of both, undoes and
    /// redoes that, and types around 777's next edit before undoing twice.
    fn edits_from_bound_yjs_editors() -> Vec<(u64, Vec<u8>)> {
        [
            (777, "0101890600040107636f6e74656e7414506c616e3a207368697020f09f9a8020736f6f6e00"),
            (
                4242,
                "010992210084890611012084922100016184922101016e84922102016484922103012084922104017484922105016584922106017384922107017400",
            ),
            (4242, "00018906010006"),
            (4242, "01019221094489060006506c616e3a2000"),
            (4242, "00019221010906"),
            (777, "0101890612c489060889060902585900"),
            (4242, "010292210fc489060789060802c3a984922108012100"),
            (4242, "00019221010f02"),
        ]
        .iter()
        .map(|(client, hex)| (*client, from_hex(hex)))
        .collect()
    }

    const TEXT_FROM_YJS: &str = " Roadmap\n\nShip it 🚀 soon.\n- step é";

    #[test]
    fn accepts_honest_edits_in_order() {
        let state = merge_in_order(&edits_from_yjs()).unwrap();
        assert_eq!(read(Some(&state)).unwrap(), TEXT_FROM_YJS);

        let first = edit(None, Edit::Append("# Plan\n\nShip it 🚀."), 1).unwrap();
        let state = merge_as_its_client(None, &first).unwrap();
        let second = edit(
            Some(&state),
            Edit::Replace {
                find: "Ship it",
                with: "Ship é",
            },
            1,
        )
        .unwrap();
        let state = merge_as_its_client(Some(&state), &second).unwrap();
        assert_eq!(read(Some(&state)).unwrap(), "# Plan\n\nShip é 🚀.");
    }

    #[test]
    fn honest_yjs_editors_add_pieces_only_for_their_own_client() {
        let mut state = None;
        for (client, update) in edits_from_bound_yjs_editors() {
            state = Some(merge(state.as_deref(), &update, client).unwrap());
        }
        assert_eq!(read(state.as_deref()).unwrap(), "shiXYp 🚀 soon and test");
    }

    #[test]
    fn refuses_pieces_that_do_not_continue_their_client() {
        let edits = edits_from_yjs();
        let once = merge_as_its_client(None, &edits[0]).unwrap();
        assert!(
            merge_as_its_client(Some(&once), &edits[0]).is_err(),
            "the same positions again"
        );
        assert!(
            merge_as_its_client(None, &edits[2]).is_err(),
            "a hole before them"
        );
        let one_then_three = "0103d2d5e5b00500040107636f6e74656e74036f6e650a0384d2d5e5b0050505746872656500";
        assert!(
            merge_as_its_client(None, &from_hex(one_then_three)).is_err(),
            "a skipped run"
        );
    }

    #[test]
    fn refuses_nested_values() {
        let mut nested_arrays = vec![1, 1, 1, 0, 8, 1, 7];
        nested_arrays.extend_from_slice(b"content");
        nested_arrays.push(1);
        for _ in 0..400_000 {
            nested_arrays.extend_from_slice(&[117, 1]);
        }
        nested_arrays.extend_from_slice(&[126, 0]);
        assert!(check_on_empty_page(&nested_arrays).is_err());
    }

    #[test]
    fn refuses_counts_larger_than_the_update() {
        let huge = [0x80, 0x80, 0x80, 0x80, 0x01];
        assert!(check_on_empty_page(&huge).is_err(), "clients");
        assert!(
            check_on_empty_page(&[1, 0x80, 0x80, 0x80, 0x01, 1, 0]).is_err(),
            "pieces"
        );
        assert!(
            check_on_empty_page(&[0, 1, 1, 0x80, 0x80, 0x80, 0x01]).is_err(),
            "deleted ranges"
        );
    }

    #[test]
    fn refuses_content_a_text_never_holds() {
        let content_of_each_kind: [(u8, &[u8]); 7] = [
            (2, &[1, 1, b'1']),                         // JSON
            (3, &[1, 0]),                               // binary
            (5, &[2, b'{', b'}']),                      // embed
            (6, &[1, b'b', 4, b't', b'r', b'u', b'e']), // format
            (7, &[0]),                                  // a nested type
            (8, &[1, 120]),                             // any
            (9, &[1, b'g', 126]),                       // a sub-document
        ];
        for (kind, content) in content_of_each_kind {
            let mut item = vec![kind, 1, 7];
            item.extend_from_slice(b"content");
            item.extend_from_slice(content);
            assert!(
                check_on_empty_page(&pieces(1, 0, &[item])).is_err(),
                "kind {kind}"
            );
        }
        let mut unused_bit = text_item("hi");
        unused_bit[0] |= 0x10;
        assert!(
            check_on_empty_page(&pieces(1, 0, &[unused_bit])).is_err(),
            "unused info bit"
        );
        assert!(check_on_empty_page(&pieces(1, 0, &[text_item("hi")])).is_ok());
    }

    #[test]
    fn refuses_items_outside_the_page_text() {
        let mut other_root = vec![4, 1, 5];
        other_root.extend_from_slice(b"other");
        other_root.extend_from_slice(&[2, b'h', b'i']);
        assert!(
            check_on_empty_page(&pieces(1, 0, &[other_root])).is_err(),
            "another root"
        );

        let mut under_a_key = text_item("hi");
        under_a_key[0] |= HAS_PARENT_SUB;
        under_a_key.splice(10..10, [1, b'k']);
        assert!(
            check_on_empty_page(&pieces(1, 0, &[under_a_key])).is_err(),
            "under a key"
        );

        let inside_an_item = vec![4, 0, 1, 0, 2, b'h', b'i'];
        assert!(
            check_on_empty_page(&pieces(1, 0, &[inside_an_item])).is_err(),
            "inside an item"
        );
    }

    #[test]
    fn refuses_numbers_yrs_cannot_hold() {
        let largest_client = (1 << 53) - 1;
        assert!(check_on_empty_page(&pieces(largest_client, 0, &[text_item("a")])).is_ok());
        assert!(
            check_on_empty_page(&pieces(largest_client + 1, 0, &[text_item("a")])).is_err(),
            "client id"
        );
        assert!(
            check_on_empty_page(&pieces(1, 1 << 32, &[text_item("a")])).is_err(),
            "clock"
        );

        let mut nearly_full = ClientEnds::default();
        nearly_full.record(1, u32::MAX - 1);
        let past_the_end = pieces(1, u64::from(u32::MAX) - 1, &[text_item("ab")]);
        assert!(
            check_update(&past_the_end, &nearly_full, 1).is_err(),
            "clock plus length"
        );
        let mut near_the_limit = ClientEnds::default();
        near_the_limit.record(1, MAX_CLOCK - 2);
        let up_to_the_limit = pieces(1, u64::from(MAX_CLOCK) - 2, &[text_item("a")]);
        assert!(check_update(&up_to_the_limit, &near_the_limit, 1).is_ok());
        let to_the_limit = pieces(1, u64::from(MAX_CLOCK) - 2, &[text_item("ab")]);
        assert!(
            check_update(&to_the_limit, &near_the_limit, 1).is_err(),
            "past clock 2^30"
        );
        assert!(
            check_on_empty_page(&deletions(&[(u64::from(u32::MAX), 1)])).is_err(),
            "deleted range end"
        );
    }

    #[test]
    fn refuses_malformed_pieces() {
        assert!(
            check_on_empty_page(&[1, 0, 1, 0, 0]).is_err(),
            "a client with no pieces"
        );
        let mut twice = vec![2];
        for clock in [0, 1] {
            twice.extend_from_slice(&[1, 1, clock]);
            twice.extend_from_slice(&text_item("a"));
        }
        twice.push(0);
        assert!(check_on_empty_page(&twice).is_err(), "a client listed twice");
        assert!(
            check_on_empty_page(&pieces(1, 0, &[text_item("")])).is_err(),
            "empty text"
        );
        let mut deleted_nothing = text_item("");
        deleted_nothing[0] = DELETED;
        assert!(
            check_on_empty_page(&pieces(1, 0, &[deleted_nothing])).is_err(),
            "deleted nothing"
        );
        assert!(
            check_on_empty_page(&pieces(1, 0, &[vec![GC, 1]])).is_err(),
            "a GC block"
        );
        assert!(
            check_on_empty_page(&pieces(1, 0, &[vec![SKIP, 1]])).is_err(),
            "a skip block"
        );
    }

    #[test]
    fn refuses_malformed_deletions() {
        assert!(check_on_empty_page(&deletions(&[(0, 2), (3, 1), (9, 1)])).is_ok());
        assert!(
            check_on_empty_page(&deletions(&[(5, 1), (0, 1)])).is_err(),
            "out of order"
        );
        assert!(
            check_on_empty_page(&deletions(&[(0, 3), (2, 1)])).is_err(),
            "overlapping"
        );
        assert!(
            check_on_empty_page(&deletions(&[(0, 2), (2, 1)])).is_err(),
            "touching"
        );
        assert!(check_on_empty_page(&deletions(&[(0, 0)])).is_err(), "empty");
        assert!(
            check_on_empty_page(&[0, 2, 1, 1, 0, 1, 1, 1, 5, 1]).is_err(),
            "a client listed twice"
        );

        let at_most = |count: u64| -> Vec<(u64, u64)> { (0..count).map(|range| (range * 2, 1)).collect() };
        assert!(check_on_empty_page(&deletions(&at_most(MAX_DELETED_RANGES))).is_ok());
        assert!(
            check_on_empty_page(&deletions(&at_most(MAX_DELETED_RANGES + 1))).is_err(),
            "too many"
        );
    }

    #[test]
    fn refuses_trailing_bytes() {
        let mut update = edits_from_yjs().remove(0);
        update.push(0);
        assert!(check_on_empty_page(&update).is_err());
    }

    /// Updates that made yrs abort, crash, hang or panic after passing an
    /// earlier, looser version of this check (independent review of #163),
    /// the last past clock 2^31, where live merges and rebuilds also
    /// disagreed on what to accept.
    #[test]
    fn updates_that_broke_yrs_are_refused() {
        let reported = [
            vec![
                "0303028080808108040107636f6e74656e74020000440000004400000001000084020008f0908080f0908080040000840000000001010107636f6e74656e7400c4000000000000",
            ],
            vec![
                "03040000010107636f6e74656e7400000184000003000000040107636f6e74656e740b00000000000000000000000402008402010b000000000000000000000041000000c10000000000410000000300000001c40000000000000103000000000000",
                "02040000410000000001c4000000000a000000000000000000006400000003020100010001c1000000000000",
            ],
            vec![
                "01030200040107636f6e74656e7403000000810000008400000000",
                "03044d00040107636f6e74656e7404f090808084020008f0908080f0908080010107636f6e74656e7400040107636f6e74656e7402c280024d0100010001040000040107636f6e74656e740600f090808000c4020100000b0000000000000000000000e40000000002c2808400000000",
            ],
            vec![
                "0101ebd7bbf585dad80c00040107636f6e74656e7419000000000000f090808000000000000000000000000000000000",
                "02030200c4000000000600f090808000c40000000008f0908080f0908080840000010005ebd7bbf585dad80c17c40000000002c2808400000600f090808000c4000000000100c400000000020000040107636f6e74656e7401000100010001",
                "02010209840000010001ea0700410000fcffffff0f0100010001",
                "0102ea07fcffffff0f040107636f6e74656e74010084ea070004f09080800100010001",
            ],
            vec![
                "0101fb86fcccb6e09f0c00040107636f6e74656e741948656c6c6f20f09f9a8020776f726c642c206120706167652e00",
                "020402008402fdffffff0702c3a9810200038403ffffffff07016184030102c3a903fb86fcccb6e09f0c174403010b68656c6c6f20776f726c648402010171814d010301020202030801",
                "01010206010107636f6e74656e74faffffff0702e80704010106060d0515010302eccdbf1404f4cdbf1401",
                "01020206410301faffffff07010107636f6e74656e7404010304ffe8de2a0385e9de2a038ae9de2a028fe9de2a04",
            ],
        ];
        for updates in reported {
            let updates: Vec<Vec<u8>> = updates
                .iter()
                .flat_map(|hex| one_update_per_client(&from_hex(hex)))
                .collect();
            assert!(merge_in_order(&updates).is_err());
        }

        let mut empty_clients = Vec::new();
        var(&mut empty_clients, 100_000);
        for client in 0..100_000 {
            empty_clients.push(0);
            var(&mut empty_clients, client);
            empty_clients.push(0);
        }
        empty_clients.push(0);
        assert!(check_on_empty_page(&empty_clients).is_err());
    }

    struct Random(u64);

    impl Random {
        fn below(&mut self, bound: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % bound
        }

        fn chance(&mut self, percent: u64) -> bool {
            self.below(100) < percent
        }

        fn clock(&mut self) -> u64 {
            match self.below(20) {
                0 => u64::from(u32::MAX) - self.below(4),
                1 => self.below(2_000),
                2 => (1 << 31) - self.below(4),
                3 => (1 << 30) - self.below(4),
                _ => self.below(30),
            }
        }
    }

    /// Hostile updates shaped like real ones: they often continue a client
    /// where it ends, with random origins, deletions and broken parts.
    fn hostile_update(random: &mut Random, ends: &ClientEnds) -> Vec<u8> {
        const TEXTS: [&str; 7] = ["a", "bc", "🚀", "é", "x🚀y", "", "hello world"];
        let mut clients: Vec<u64> = vec![1, 2, 3, 77];
        clients.extend(ends.0.keys());
        let pick_client = |random: &mut Random| clients[random.below(clients.len() as u64) as usize];
        let mut update = Vec::new();
        let sections = random.below(3);
        var(&mut update, sections);
        for _ in 0..sections {
            let client = pick_client(random);
            let blocks = 1 + random.below(5);
            var(&mut update, blocks);
            var(&mut update, client);
            let clock = if random.chance(80) {
                u64::from(ends.end_of(client))
            } else {
                random.clock()
            };
            var(&mut update, clock);
            for _ in 0..blocks {
                if random.chance(5) {
                    update.push(if random.chance(50) { GC } else { SKIP });
                    var(&mut update, 1 + random.below(5));
                    continue;
                }
                let has_origin = random.chance(60);
                let has_right_origin = random.chance(40);
                let is_deleted = random.chance(25);
                let mut info = if is_deleted { DELETED } else { STRING };
                if has_origin {
                    info |= HAS_ORIGIN;
                }
                if has_right_origin {
                    info |= HAS_RIGHT_ORIGIN;
                }
                update.push(info);
                for _ in 0..u8::from(has_origin) + u8::from(has_right_origin) {
                    var(&mut update, pick_client(random));
                    var(&mut update, random.clock());
                }
                if !has_origin && !has_right_origin {
                    update.extend_from_slice(&[1, 7]);
                    update.extend_from_slice(b"content");
                }
                if is_deleted {
                    let length = if random.chance(10) {
                        random.clock().saturating_sub(clock)
                    } else {
                        random.below(6)
                    };
                    var(&mut update, length);
                } else {
                    let text = TEXTS[random.below(TEXTS.len() as u64) as usize];
                    var(&mut update, text.len() as u64);
                    update.extend_from_slice(text.as_bytes());
                }
            }
        }
        let deleted_clients = random.below(3);
        var(&mut update, deleted_clients);
        for _ in 0..deleted_clients {
            var(&mut update, pick_client(random));
            let ranges = random.below(5);
            var(&mut update, ranges);
            let mut start = 0;
            for _ in 0..ranges {
                start = if random.chance(80) {
                    start + 1 + random.below(8)
                } else {
                    random.clock()
                };
                let length = random.below(8);
                var(&mut update, start.min(u64::from(u32::MAX) - length));
                var(&mut update, length);
                start += length;
            }
        }
        if random.chance(10) {
            let at = random.below(update.len() as u64) as usize;
            update[at] ^= 1 << random.below(8);
        }
        update
    }

    /// Hostile and honest updates, merged one after another as they arrive
    /// and replayed as a rebuild does: neither panics or hangs, both accept
    /// the same updates, and they end with the same text unless a hostile
    /// update got in (its origins can contradict each other, #186).
    #[test]
    fn live_merges_and_rebuilds_agree_on_hostile_updates() {
        let started = Instant::now();
        let mut random = Random(0x5eed_cafe);
        let mut accepted = 0;
        for _ in 0..1_500 {
            let opening = edit(None, Edit::Append("Hello 🚀 world"), 1).unwrap();
            let mut state = merge_as_its_client(None, &opening).unwrap();
            let mut replay = PageMerge::default();
            replay.add(&opening, first_client(&opening)).unwrap();
            let mut sent = vec![opening];
            let mut is_hostile_accepted = false;
            for _ in 0..1 + random.below(6) {
                let is_honest = random.chance(30);
                let update = if is_honest {
                    let change = if random.chance(50) {
                        Edit::Append("more")
                    } else {
                        Edit::Replace { find: "w", with: "W" }
                    };
                    edit(Some(&state), change, 1)
                        .unwrap_or_else(|_| edit(Some(&state), Edit::Append("z"), 1).unwrap())
                } else {
                    hostile_update(&mut random, &ClientEnds::of_state(&state).unwrap())
                };
                let live = merge_as_its_client(Some(&state), &update);
                let replayed = replay.add(&update, first_client(&update));
                assert_eq!(live.is_ok(), replayed.is_ok(), "update {update:02x?}");
                if let Ok(next) = live {
                    state = next;
                    accepted += 1;
                    is_hostile_accepted |= !is_honest;
                    sent.push(update);
                }
            }
            if is_hostile_accepted {
                continue;
            }
            let hex: Vec<String> = sent
                .iter()
                .map(|update| update.iter().map(|byte| format!("{byte:02x}")).collect())
                .collect();
            assert_eq!(
                read(Some(&state)).unwrap(),
                read(Some(&replay.state())).unwrap(),
                "accepted updates: {hex:?}"
            );
        }
        assert!(accepted > 1_000, "only {accepted} updates were accepted");
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "took {:?}",
            started.elapsed()
        );
    }
}
