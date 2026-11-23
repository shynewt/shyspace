//! Packs: each author uploads their own log again in aligned ranges of 16,
//! 256 and 4096 ops, as ordinary segments, so a reader catching up fetches a
//! few wide objects instead of one per upload. A pack is only a container:
//! every op in it is checked like any other, and nothing is overwritten.

use std::collections::{BTreeMap, HashSet};

use anyhow::{Context, Result, ensure};
use iroh::PublicKey;

use super::replicate::SegmentsRead;
use crate::{
    Workspace,
    transport::bucket::{ListedObject, MAX_OBJECT_SIZE, SegmentName},
};

const PACK_WIDTHS: [u64; 3] = [16, 256, 4096];
const PACKED_THROUGH_META: &str = "packed_through";
/// A segment holds a count (at most 10 bytes) before its envelopes.
const COUNT_BYTES: u64 = 10;
/// Chain objects that may fail before the chain is given up and the rest is
/// read in listing order, several at a time. Names planted to claim wide
/// ranges sort last there, after the honest segments, so they can't each
/// cost a wait or waste the downloads started after them.
const MAX_CHAIN_FAILURES: usize = 3;

/// For each boundary of 16 ops pushed past `packed_through`, in order, the
/// aligned ranges ending there, narrowest first.
fn packs_due(packed_through: u64, pushed: u64) -> Vec<(u64, Vec<(u64, u64)>)> {
    let narrowest = PACK_WIDTHS[0];
    let mut due = Vec::new();
    let mut boundary = (packed_through / narrowest + 1) * narrowest;
    while boundary <= pushed {
        let ranges = PACK_WIDTHS
            .iter()
            .filter(|width| boundary.is_multiple_of(**width))
            .map(|width| (boundary - width + 1, boundary))
            .collect();
        due.push((boundary, ranges));
        boundary += narrowest;
    }
    due
}

/// The objects covering what follows `held` in the fewest downloads: from
/// each point, the one reaching furthest, the narrower on a tie.
fn covering_chain(
    held: u64,
    candidates: &[(ListedObject, SegmentName)],
    tried: &HashSet<String>,
) -> Vec<ListedObject> {
    let mut chain = Vec::new();
    let mut reached = held;
    loop {
        let next = candidates
            .iter()
            .filter(|(object, name)| {
                name.first <= reached.saturating_add(1) && name.last > reached && !tried.contains(&object.key)
            })
            .max_by_key(|(_, name)| (name.last, name.first));
        let Some((object, name)) = next else {
            return chain;
        };
        chain.push(object.clone());
        reached = name.last;
    }
}

/// Splits `objects`, each reaching past `held`, into those whose part past
/// `held` no strictly narrower ones cover, and those covered. Names only
/// order the reading: a covered object is still read if the others turn out
/// not to hold its ops.
fn split_covered(
    held: u64,
    mut objects: Vec<(ListedObject, SegmentName)>,
) -> (Vec<ListedObject>, Vec<(ListedObject, SegmentName)>) {
    objects.sort_by_key(|(_, name)| name.last - name.first);
    let mut covered_ranges = BTreeMap::new();
    let (mut uncovered, mut covered) = (Vec::new(), Vec::new());
    while let Some((_, narrowest)) = objects.first() {
        let width = narrowest.last - narrowest.first;
        let same_width = objects
            .iter()
            .take_while(|(_, name)| name.last - name.first == width)
            .count();
        let group: Vec<_> = objects.drain(..same_width).collect();
        let ranges: Vec<(u64, u64)> = group
            .iter()
            .map(|(_, name)| (name.first.max(held.saturating_add(1)), name.last))
            .collect();
        for ((object, name), (first, last)) in group.into_iter().zip(&ranges) {
            if is_covered(&covered_ranges, *first, *last) {
                covered.push((object, name));
            } else {
                uncovered.push(object);
            }
        }
        for (first, last) in ranges {
            add_range(&mut covered_ranges, first, last);
        }
    }
    uncovered.sort_by(|left, right| left.key.cmp(&right.key));
    covered.sort_by(|(left, _), (right, _)| left.key.cmp(&right.key));
    (uncovered, covered)
}

/// Whether `ranges` (disjoint, not touching, by first seq) cover `first..=last`.
fn is_covered(ranges: &BTreeMap<u64, u64>, first: u64, last: u64) -> bool {
    ranges
        .range(..=first)
        .next_back()
        .is_some_and(|(_, &end)| end >= last)
}

fn add_range(ranges: &mut BTreeMap<u64, u64>, first: u64, last: u64) {
    let (mut first, mut last) = (first, last);
    if let Some((&start, &end)) = ranges.range(..=first).next_back()
        && end.saturating_add(1) >= first
    {
        ranges.remove(&start);
        (first, last) = (start, last.max(end));
    }
    while let Some((&start, &end)) = ranges.range(first..).next()
        && start <= last.saturating_add(1)
    {
        ranges.remove(&start);
        last = last.max(end);
    }
    ranges.insert(first, last);
}

/// What reading along the covering chain found.
pub(super) struct ChainRead {
    pub read: SegmentsRead,
    /// Keys of the objects fetched, which the rest of the read leaves out.
    pub tried: HashSet<String>,
}

impl Workspace {
    /// Where packing resumes. A store that never packed (a restored device)
    /// starts from what it has already pushed rather than packing its whole
    /// log again: those packs were written before.
    pub(super) fn packed_through(&self) -> Result<u64> {
        let state = self.state();
        if let Some(saved) = state.store.meta(PACKED_THROUGH_META)? {
            let bytes = <[u8; 8]>::try_from(saved.as_slice())
                .map_err(|_| anyhow::anyhow!("unreadable packing mark"))?;
            return Ok(u64::from_le_bytes(bytes));
        }
        let pushed = state.store.last_pushed_seq(&self.me())?;
        state.store.set_meta(PACKED_THROUGH_META, &pushed.to_le_bytes())?;
        Ok(pushed)
    }

    /// Uploads the packs due since `packed_through`. Packs only speed up
    /// reading, so one that fails is reported (the first error is returned)
    /// but not tried again: readers use the narrower packs or segments it
    /// would have covered. A bucket that refuses large uploads therefore
    /// can't make packs pile up or stop later ones.
    pub(super) async fn upload_due_packs(&self, packed_through: u64) -> Result<()> {
        let me = self.me();
        let pushed = self.state().store.last_pushed_seq(&me)?;
        let mut first_failure = None;
        for (boundary, ranges) in packs_due(packed_through, pushed) {
            for (first, last) in ranges {
                if let Err(error) = self.upload_pack(&me, first, last).await {
                    first_failure.get_or_insert(error);
                }
            }
            self.state()
                .store
                .set_meta(PACKED_THROUGH_META, &boundary.to_le_bytes())?;
        }
        first_failure.map_or(Ok(()), Err)
    }

    /// Uploads our ops `first` to `last` as one segment, unless they are too
    /// large for one object: readers then use their segments.
    async fn upload_pack(&self, me: &PublicKey, first: u64, last: u64) -> Result<()> {
        let size = self.state().store.envelope_bytes_between(me, first, last)?;
        if COUNT_BYTES + size > MAX_OBJECT_SIZE {
            return Ok(());
        }
        let envelopes = self.state().store.envelopes_between(me, first, last)?;
        ensure!(
            u64::try_from(envelopes.len())? == last - first + 1,
            "our own log is missing ops between {first} and {last}"
        );
        self.bucket()
            .put_segment(&envelopes)
            .await
            .with_context(|| format!("uploading the pack of our ops {first} to {last}"))
    }

    /// Reads `author`'s log past what we hold along the covering chain of
    /// `candidates`, before anything else is fetched. An object that doesn't
    /// settle (junk, or ops that can't apply yet) is left out and the chain
    /// planned again from where reading got to, up to `MAX_CHAIN_FAILURES`.
    pub(super) async fn read_covering_chain(
        &self,
        author: &PublicKey,
        candidates: &[(ListedObject, SegmentName)],
    ) -> Result<ChainRead> {
        let mut chain_read = ChainRead {
            read: SegmentsRead::default(),
            tried: HashSet::new(),
        };
        let mut failures = 0;
        while failures < MAX_CHAIN_FAILURES {
            let held = self.state().store.last_seq(author)?;
            let chain = covering_chain(held, candidates, &chain_read.tried);
            if chain.is_empty() {
                break;
            }
            let mut fetch = self.bucket().fetch_segments(author, chain);
            while let Some(segment) = fetch.next().await? {
                chain_read.tried.insert(segment.listed.key.clone());
                let read = self.take_segment(author, segment, &mut chain_read.read)?;
                if read.is_stopped {
                    chain_read.read.is_stopped = true;
                    return Ok(chain_read);
                }
                if !read.is_settled {
                    failures += 1;
                    break;
                }
            }
        }
        Ok(chain_read)
    }

    /// Reads `objects` of `author`'s, each reaching past what we hold: first
    /// those no narrower ones cover, then each covered one only if it still
    /// holds ops we lack. Honest packs repeat what segments hold, so when the
    /// chain was given up this reads the log about once, not several times.
    pub(super) async fn read_rest(
        &self,
        author: &PublicKey,
        objects: Vec<(ListedObject, SegmentName)>,
    ) -> Result<SegmentsRead> {
        let held = self.state().store.last_seq(author)?;
        let (uncovered, covered) = split_covered(held, objects);
        let mut read = self.read_segments(author, uncovered).await?;
        if read.is_stopped {
            return Ok(read);
        }
        let held = self.state().store.last_seq(author)?;
        let still_needed = covered
            .into_iter()
            .filter(|(_, name)| name.last > held)
            .map(|(object, _)| object)
            .collect();
        read.absorb(self.read_segments(author, still_needed).await?);
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_are_due_at_each_aligned_boundary_crossed_narrowest_first() {
        assert!(packs_due(0, 15).is_empty());
        assert_eq!(packs_due(0, 40), [(16, vec![(1, 16)]), (32, vec![(17, 32)])]);
        assert_eq!(packs_due(7, 16), [(16, vec![(1, 16)])]);
        assert_eq!(
            packs_due(4080, 4096),
            [(4096, vec![(4081, 4096), (3841, 4096), (1, 4096)])]
        );
        assert!(packs_due(16, 31).is_empty());
    }

    fn listed(first: u64, last: u64) -> (ListedObject, SegmentName) {
        let object = ListedObject {
            key: format!("{first}-{last}"),
            version: String::new(),
        };
        let name = SegmentName {
            first,
            last,
            nonce: [0; 16],
            hash: blake3::hash(b""),
        };
        (object, name)
    }

    #[test]
    fn objects_that_narrower_ones_cover_are_read_last() {
        let objects = vec![
            listed(1, 16),
            listed(1, 3),
            listed(4, 9),
            listed(10, 16),
            listed(17, 32),
            listed(17, 20),
            listed(1, 1_000_000),
            listed(10, 16),
        ];
        let (uncovered, covered) = split_covered(0, objects);
        let covered: Vec<_> = covered.into_iter().map(|(object, _)| object).collect();

        assert_eq!(
            keys(&uncovered),
            ["1-1000000", "1-3", "10-16", "10-16", "17-20", "17-32", "4-9"]
        );
        assert_eq!(keys(&covered), ["1-16"]);
        let (_, covered_past_held) = split_covered(5, vec![listed(1, 16), listed(6, 9), listed(10, 16)]);
        assert_eq!(
            covered_past_held.len(),
            1,
            "only the part past what is held counts"
        );
    }

    fn keys(chain: &[ListedObject]) -> Vec<&str> {
        chain.iter().map(|object| object.key.as_str()).collect()
    }

    #[test]
    fn the_chain_takes_the_furthest_reach_from_each_point_and_the_narrower_on_a_tie() {
        let candidates = [
            listed(1, 3),
            listed(1, 16),
            listed(4, 16),
            listed(16, 16),
            listed(17, 20),
            listed(21, 22),
            listed(24, 30),
            listed(1, 256),
        ];
        let none_tried = HashSet::new();

        assert_eq!(keys(&covering_chain(0, &candidates, &none_tried)), ["1-256"]);
        assert_eq!(keys(&covering_chain(3, &candidates[..4], &none_tried)), ["4-16"]);
        assert_eq!(
            keys(&covering_chain(16, &candidates[..7], &none_tried)),
            ["17-20", "21-22"],
            "stops at a gap"
        );
        let tried = HashSet::from(["1-256".to_string()]);
        assert_eq!(
            keys(&covering_chain(0, &candidates, &tried)),
            ["1-16", "17-20", "21-22"]
        );
    }
}
