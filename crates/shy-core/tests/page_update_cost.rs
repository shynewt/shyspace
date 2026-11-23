//! A page update must cost memory and time in proportion to its size, even
//! when it is made of many tiny text pieces that yrs splits or joins (#185).
//! Lives in its own test binary because it measures the whole process's
//! memory.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use shy_core::pages::{PageMerge, merge, read};

/// Tests here run one at a time, so each measures only its own work. A
/// failed test leaves the lock poisoned, which doesn't matter to the next.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

const CLIENT: u64 = 7;

fn var(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Pieces of `text` for one client, in Yjs's v1 encoding. The first starts
/// the page text; `origins(clock)` names the left and right neighbours of
/// every later one.
fn typed_pieces(count: u64, text: &[u8], origins: impl Fn(u64) -> (Option<u64>, Option<u64>)) -> Vec<u8> {
    let mut update = vec![1];
    var(&mut update, count);
    var(&mut update, CLIENT);
    var(&mut update, 0);
    update.extend_from_slice(&[4, 1, 7]);
    update.extend_from_slice(b"content");
    var(&mut update, text.len() as u64);
    update.extend_from_slice(text);
    for piece in 1..count {
        let (origin, right_origin) = origins(piece * text.len() as u64);
        update.push(4 | origin.map_or(0, |_| 0x80) | right_origin.map_or(0, |_| 0x40));
        for neighbour in [origin, right_origin].into_iter().flatten() {
            var(&mut update, CLIENT);
            var(&mut update, neighbour);
        }
        var(&mut update, text.len() as u64);
        update.extend_from_slice(text);
    }
    update.push(0);
    update
}

fn one_character_pieces(count: u64, origins: impl Fn(u64) -> (u64, Option<u64>)) -> Vec<u8> {
    typed_pieces(count, b"a", |clock| {
        let (origin, right_origin) = origins(clock);
        (Some(origin), right_origin)
    })
}

/// An update deleting the client's characters in `ranges` (start, length).
fn deleting(ranges: &[(u64, u64)]) -> Vec<u8> {
    let mut update = vec![0, 1];
    var(&mut update, CLIENT);
    var(&mut update, ranges.len() as u64);
    for &(start, length) in ranges {
        var(&mut update, start);
        var(&mut update, length);
    }
    update
}

/// The most memory the process held in RAM since the last reset, in bytes.
#[cfg(target_os = "linux")]
fn peak_resident_bytes() -> usize {
    let status = std::fs::read_to_string("/proc/self/status").expect("Linux reports memory use");
    let kilobytes = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.trim().strip_suffix("kB"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .expect("the status has a VmHWM line in kB");
    kilobytes << 10
}

#[cfg(target_os = "linux")]
fn reset_peak_resident_bytes() {
    std::fs::write("/proc/self/clear_refs", "5").expect("Linux resets the peak on request");
}

#[cfg(target_os = "linux")]
#[test]
fn a_long_run_of_typed_pieces_merges_in_linear_memory() {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let pieces = 40_000;
    let typed_one_after_another = one_character_pieces(pieces, |clock| (clock - 1, None));

    reset_peak_resident_bytes();
    let before = peak_resident_bytes();
    let state = merge(None, &typed_one_after_another, CLIENT).expect("the update is accepted");
    let grown = peak_resident_bytes().saturating_sub(before);

    // yrs 0.28.1 needed about 0.9 GB here: every join copied all the text
    // joined so far.
    assert!(grown < 64 << 20, "grew by {} MB", grown >> 20);
    assert_eq!(read(Some(&state)).expect("the page reads").len(), pieces as usize);
}

/// `update` (from [`one_character_pieces`]) also deleting the client's
/// pieces in `clocks`.
fn deleting_own_pieces(mut update: Vec<u8>, clocks: std::ops::Range<u64>) -> Vec<u8> {
    update.pop();
    update.push(1);
    var(&mut update, CLIENT);
    update.push(1);
    var(&mut update, clocks.start);
    var(&mut update, clocks.end - clocks.start);
    update
}

/// Time to merge `update` into an empty page and check its text's length.
fn time_to_merge(update: &[u8], characters: u64) -> Duration {
    time_to_merge_into(None, update, characters)
}

fn time_to_merge_into(page: Option<&[u8]>, update: &[u8], characters: u64) -> Duration {
    let started = Instant::now();
    let state = merge(page, update, CLIENT).expect("the update is accepted");
    let took = started.elapsed();
    assert_eq!(
        read(Some(&state)).expect("the page reads").len(),
        characters as usize
    );
    took
}

/// How long merging `update` takes compared with merging `baseline`: the
/// faster of two tries each, taken in turns, so neither gets a colder start.
fn times_to_merge(update: (&[u8], u64), baseline: (&[u8], u64)) -> (Duration, Duration) {
    let mut fastest = (Duration::MAX, Duration::MAX);
    for _ in 0..2 {
        fastest.1 = fastest.1.min(time_to_merge(baseline.0, baseline.1));
        fastest.0 = fastest.0.min(time_to_merge(update.0, update.1));
    }
    fastest
}

#[test]
fn many_short_runs_of_pieces_merge_in_linear_time() {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let pieces = 360_001;
    // Pairs of pieces, each pair typed right after the first piece, so the
    // two in a pair join but pairs don't.
    let joining_pairs = one_character_pieces(pieces, |clock| {
        let pair_start = clock - (clock - 1) % 2;
        let previous_pair = (pair_start > 1).then(|| pair_start - 2);
        if clock == pair_start {
            (0, previous_pair)
        } else {
            (pair_start, previous_pair)
        }
    });
    // The same pieces, each typed right after the first piece: nothing joins.
    let separate = one_character_pieces(pieces, |clock| (0, (clock > 1).then(|| clock - 1)));
    assert!(joining_pairs.len() < 4_000_000, "fits in one page op");

    let (with_joins, without_joins) = times_to_merge((&joining_pairs, pieces), (&separate, pieces));

    // yrs 0.28.1 shifted the rest of the client's piece list after every
    // join: about five times as long as with no joins in a test build, twenty
    // in a release build. Comparing the two keeps the check independent of
    // the machine's speed.
    assert!(
        with_joins < without_joins * 2,
        "{with_joins:?} with joins, {without_joins:?} without"
    );
}

#[test]
fn pieces_typed_and_deleted_in_one_update_merge_in_linear_time() {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let typed = 200_000;
    let pieces = 2 * typed;
    // A run typed one after another, then pieces each typed right after the
    // run, which don't join with anything.
    let kept = one_character_pieces(pieces, |clock| match clock < typed {
        true => (clock - 1, None),
        false => (typed - 1, (clock > typed).then(|| clock - 1)),
    });
    let run_deleted = deleting_own_pieces(kept.clone(), 0..typed);
    assert!(run_deleted.len() < 4_000_000, "fits in one page op");

    let (with_deletion, without_deletion) = times_to_merge((&run_deleted, pieces - typed), (&kept, pieces));

    // yrs 0.28.1 joined the deleted pieces one pair at a time, shifting the
    // rest of the client's piece list after each.
    assert!(
        with_deletion < without_deletion * 2,
        "{with_deletion:?} with the deletion, {without_deletion:?} without"
    );
}

#[test]
fn deletions_that_split_many_pieces_merge_in_linear_time() {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let pieces = 280_000;
    // Two characters each, each typed at the start of the page, so none join.
    let typed = typed_pieces(pieces, b"ab", |clock| (None, Some(clock - 2)));
    let page = merge(None, &typed, CLIENT).expect("the update is accepted");
    let deletions = 100_000;
    // One character of every other piece, so each deletion splits a piece.
    let splitting = deleting(&(0..deletions).map(|piece| (4 * piece + 1, 1)).collect::<Vec<_>>());
    // The same pieces whole: nothing splits.
    let whole = deleting(&(0..deletions).map(|piece| (4 * piece, 2)).collect::<Vec<_>>());
    let left = 2 * pieces - deletions;

    let mut fastest = (Duration::MAX, Duration::MAX);
    for _ in 0..2 {
        fastest.1 = fastest
            .1
            .min(time_to_merge_into(Some(&page), &whole, left - deletions));
        fastest.0 = fastest.0.min(time_to_merge_into(Some(&page), &splitting, left));
    }
    let (with_splits, without_splits) = fastest;

    // yrs 0.28.1 shifted the rest of the client's piece list after every
    // split.
    assert!(
        with_splits < without_splits * 2,
        "{with_splits:?} with splits, {without_splits:?} without"
    );
}

#[test]
fn deletions_inside_one_long_piece_merge_in_linear_time() {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let deletions = 40_000;
    let piece_length = 50;
    let characters = deletions * piece_length;
    let pasted = typed_pieces(1, &vec![b'a'; characters as usize], |_| (None, None));
    // The same text as many short pieces, each typed at the start of the
    // page, so none join.
    let short = vec![b'a'; piece_length as usize];
    let typed = typed_pieces(deletions, &short, |clock| (None, Some(clock - piece_length)));
    let long_page = merge(None, &pasted, CLIENT).expect("the update is accepted");
    let short_page = merge(None, &typed, CLIENT).expect("the update is accepted");
    // One character inside every stretch of `piece_length`.
    let ranges: Vec<(u64, u64)> = (0..deletions)
        .map(|stretch| (stretch * piece_length + 1, 1))
        .collect();
    let scattered = deleting(&ranges);
    let left = characters - deletions;

    let mut fastest = (Duration::MAX, Duration::MAX);
    for _ in 0..2 {
        fastest.1 = fastest
            .1
            .min(time_to_merge_into(Some(&short_page), &scattered, left));
        fastest.0 = fastest
            .0
            .min(time_to_merge_into(Some(&long_page), &scattered, left));
    }
    let (in_one_piece, in_many_pieces) = fastest;

    // yrs 0.28.1 copied the rest of the piece's text at every split.
    assert!(
        in_one_piece < in_many_pieces * 2,
        "{in_one_piece:?} in one piece, {in_many_pieces:?} in many"
    );
}

/// One piece of `text` for the client, starting at `clock` and typed at the
/// start of the page, whose first piece starts at `first_clock`.
fn piece_typed_at_start(clock: u64, first_clock: u64, text: &[u8]) -> Vec<u8> {
    let mut update = vec![1, 1];
    var(&mut update, CLIENT);
    var(&mut update, clock);
    update.push(0x44);
    var(&mut update, CLIENT);
    var(&mut update, first_clock);
    var(&mut update, text.len() as u64);
    update.extend_from_slice(text);
    update.push(0);
    update
}

#[test]
fn small_deletions_cost_nothing_for_text_they_leave_alone() {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let edits = 1_000;
    let short = typed_pieces(1, &vec![b'a'; 3 * edits], |_| (None, None));
    let long = piece_typed_at_start(3 * edits as u64, 0, &vec![b'b'; 500_000]);
    // Each takes out one character of the short piece, splitting it twice.
    let deletions: Vec<Vec<u8>> = (0..edits as u64)
        .map(|edit| deleting(&[(3 * edit + 1, 1)]))
        .collect();
    let time_to_replay = |with_long_piece: bool| {
        let mut replay = PageMerge::default();
        replay.add(&short, CLIENT).expect("the update is accepted");
        if with_long_piece {
            replay.add(&long, CLIENT).expect("the update is accepted");
        }
        let started = Instant::now();
        for deletion in &deletions {
            replay.add(deletion, CLIENT).expect("the update is accepted");
        }
        started.elapsed()
    };

    let mut fastest = (Duration::MAX, Duration::MAX);
    for _ in 0..2 {
        fastest.1 = fastest.1.min(time_to_replay(false));
        fastest.0 = fastest.0.min(time_to_replay(true));
    }
    let (with_long_piece, without) = fastest;

    // An earlier version of the patched yrs copied every piece after the
    // first split, split or not.
    assert!(
        with_long_piece < without * 2 + Duration::from_millis(50),
        "{with_long_piece:?} with a long piece after them, {without:?} without"
    );
}
