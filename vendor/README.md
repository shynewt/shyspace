# Patched dependencies

## yrs

`yrs/` is yrs 0.28.1 exactly as published on crates.io (upstream commit `0ab918a`), with one fix. The root `Cargo.toml` uses it through `[patch.crates-io]`, and `crates/shy-core` pins `yrs = "=0.28.1"` so the copy can't be skipped silently.

**Why:** when yrs splits or joins text pieces, its cost grew with the square of the pieces. Any member could send one page update that freezes or kills every device that merges it (#185):

- one long run of pieces typed one after another copied all the text joined so far at every join (100,000 pieces: 5 GB; 800,000: about 31 GB);
- many short runs shifted the rest of the client's piece list after every run (400,000 pieces: 6 s);
- pieces typed and deleted in the same update were joined one pair at a time, shifting the rest of the list after each pair, and copying text like the first case (400,000 pieces: 14 s, or many GB).
- deletions that split pieces shifted the rest of the list after every split (1,120,000 pieces, then 100,000 splitting deletions: 89 s).

**What changed** (in `src/block.rs`, `src/block_store.rs`, `src/id_set.rs`, `src/transaction.rs`):

- `Block::can_squash`, `Item::can_squash` and `ItemContent::can_squash` check whether two pieces would join, without changing them;
- `ClientBlockList::squash_left` finds the start of the run first, then joins left to right, so each piece's text is appended once;
- `TransactionMut::apply_delete` first makes the splits a client's deletions need with `TransactionMut::split_before_delete` and `ClientBlockList::split_items_at`, in one pass that rebuilds the list once. It only takes splits it is sure the deleting loop would make (sorted ranges with gaps, nothing nested under a deleted item); the loop still makes any others the old, slow way;
- the end of a transaction joins a client's new pieces with `ClientBlockList::squash_left_from`, and its deleted pieces with `ClientBlockList::squash_left_in` (replacing `squash_left_range_compaction`): each is one pass that compacts the list once.

Pieces split and join exactly as before; only the cost changed. `crates/shy-core/tests/page_update_cost.rs` guards it, and yrs's own test suite (403 tests, run from the upstream repository at the same commit) passes before and after.

Two other additions: `LICENSE`, copied from the upstream repository since the published crate leaves it out, and a `[lints]` table at the end of `Cargo.toml` so yrs's own warnings don't fill our builds.

**Dropping the copy:** once a yrs release has the fix, delete `vendor/yrs`, this section and the `[patch.crates-io]` entry, set the version in `crates/shy-core/Cargo.toml`, and run every check, including `page_update_cost`.

To see the change: `diff -r <the yrs-0.28.1 crate in cargo's registry/src> vendor/yrs`.
