# Working on shyspace

Rules for every agent and contributor. shyspace is a real tool people rely on for private conversations, so the bar is production quality: correct, tested, secure, simple.

## Before you start

- Read `docs/README.md`. It links to the architecture, security model, decisions and development guide.
- Check open issues (`gh issue list`) for context on what you're touching.
- Don't assume. The code and command output are the truth: look before you rely on a function, flag, path or behavior. Search with `rg -n`, then read only the lines you need.
- Before calling something unused, search for every way it could be reached (other names, re-exports, Tauri commands, MCP tools, the UI's `invoke` names).

## How to work

1. **Understand** the change and where it belongs (see "Where code goes").
2. **Write a failing test first** (TDD). For a bug, the test reproduces it. For a feature, it describes the behavior from the outside.
3. **Make it pass** with the simplest code that works.
4. **Clean up**: names, duplication, file size, comments. Tests stay green.
5. **Run every check** (below) and read your own diff.
6. **Update docs** in the same commit if behavior, architecture or security changed.

Keep the diff to what the task needs. If you notice an unrelated problem, open an issue instead of fixing it on the side.

## Principles

- **KISS:** the simplest design that fully solves the problem. No cleverness that needs explaining.
- **YAGNI:** build for today's real needs, not imagined ones. No unused options, hooks or layers.
- **DRY, with judgment:** extract shared code on the third repetition, not the second. Two similar lines are cheaper than a wrong abstraction.
- **Don't reinvent the wheel:** use mature libraries for hard problems (crypto, CRDTs, networking, parsing). Never write our own cryptography.
- **Fail loudly:** never swallow errors, add silent fallbacks, or default values that hide a failure. If something can't work, say so with a clear error.
- **Delete** dead code and commented-out code. Git remembers.

## Where code goes

| Place | Owns | Must not |
|---|---|---|
| `crates/shy-core` | all logic: crypto, membership, sync, storage, merging, search | know about any UI, CLI or MCP |
| `crates/shy-cli` | argument parsing, MCP tool definitions, terminal output | contain business logic |
| `apps/desktop/src-tauri` | one Tauri command per `Workspace` call, event forwarding | contain business logic |
| `apps/desktop/src/lib` | the `invoke` bridge (`api.ts`), hooks, pure helpers | render UI |
| `apps/desktop/src/components` | one component per file, UI only | call `invoke` directly (go through `api.ts`) |

Inside `shy-core`, each file has one job (see the file map in `docs/architecture.md`). A new concern gets a new module. Past ~400 lines, split the file by responsibility.

## Code style

**Names** say what things are and do, so code reads without comments (`epoch_key`, not `ek`). Functions are verbs; booleans read as `is_`/`has_`/`should_`. No abbreviations except universal ones (`id`, `url`, `db`).

**Functions** do one thing. If the description needs "and", split it. No boolean flag parameters (`send(true)`): use two functions or an enum. Prefer early returns and guard clauses over nesting deeper than about three levels.

### Rust

- Errors: `anyhow::Result` with `.context(...)` that says what was being attempted. Use `ensure!`/`bail!` for checks.
- No `unwrap`, `dbg!`, `todo!`, `unimplemented!` or `unsafe` outside tests; lints deny them. `expect` only for true invariants, with a message saying why it can't fail.
- Treat everything from gossip, the bucket or another member as hostile: verify before storing, bound sizes and counts, never panic on it.
- Never hold a lock across `.await`. Keep `Mutex` sections short and synchronous.
- SQL is always parameterized. Schema changes need a migration plan and an issue first.
- Every op type must merge the same way in any order and when applied twice. Add a test proving it.

### TypeScript and React

- `strict` TypeScript. No `any`, no `as` casts to silence errors, no non-null `!` except on values the code just checked.
- Types for core data live in `lib/api.ts` and must match the Rust `views.rs` exactly.
- Data loading goes through `useLive`; local state stays in the component that needs it.
- Colors, shadows and fonts come from the tokens in `styles.css`. No raw hex values in components (the brand logo is the one exception).
- Animations use Motion, stay short (under ~250 ms for UI feedback), and respect reduced motion (`MotionConfig` handles it; don't override it).
- Anything rendered from message or page content is untrusted: sanitize HTML (`lib/markdown.ts`) and never use `dangerouslySetInnerHTML` on unsanitized input.
- Every interactive element is reachable by keyboard and has an accessible label.

## Comments

- Default to none. Write one only for what the code can't say: a hidden invariant, a non-obvious reason, or an outside constraint.
- Never restate the code, narrate steps, or mention callers or tests ("used by X"); those rot.
- Module docs (`//!`) explain a file's role in a few lines. Doc comments on public items explain behavior and guarantees, not the signature.
- No `TODO` comments. Open an issue instead.

## Tests

- Every behavior change comes with a test; every bug fix starts with a failing test.
- Test behavior through public APIs, not private details. Name tests after the behavior (`removed_member_cannot_read_new_messages`).
- Never weaken, skip or delete a test, loosen an assertion, or add a lint suppression to get green. Fix the code, or stop and ask.
- Tests must be deterministic. If one fails only sometimes, treat that as a real bug, not as flakiness.
- Where tests live:
  - unit tests next to the code (`#[cfg(test)] mod tests`);
  - multi-member scenarios in `crates/shy-core/src/workspace_tests.rs`, using an in-memory bucket;
  - attacks in its `hardening` module: every security rule has a test that tries to break it;
  - real processes, network and bucket in `scripts/e2e_mcp.py`.

## Checks

All must pass before every commit; never commit red:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
(cd apps/desktop && pnpm typecheck)
```

For changes to sync, networking or the CLI, also run `python3 scripts/e2e_mcp.py` (needs the local bucket; see `docs/development.md`). For UI changes, run the app and check the change by hand.

"Done" means you ran these and saw them pass. If a check fails or you couldn't run it, say so and show the error.

## Security

- Any change to crypto, membership, sync, invites or what the bucket stores:
  - adds an attack test to the `hardening` tests;
  - updates `docs/security.md`;
  - gets an independent review (a separate agent or person) before merging.
- Never log secrets, keys, invite codes or message content.
- New dependencies: ask first. Prefer mature, maintained libraries; pin to a major version.

## Docs and issues

- `docs/` must always describe the code as it is. Update it in the same commit as the change. Short, plain English, only what's worth keeping.
- Record choices someone might question in `docs/decisions.md`.
- Plans, bugs and ideas go in GitHub issues, not in docs or comments.

## Commits

- One focused commit per logical change, only once it has settled.
- Stage files by path. Never `git add -A` or `git add .`.
- Title: Conventional Commits (`feat(core): …`, `fix(desktop): …`), imperative, at most 72 characters.
- Body (optional): one to three plain lines on why, not what.
- No `Co-Authored-By` or other tool attribution trailers.

## Local files

- `.local/` is gitignored scratch space: notes, logs, screenshots, test runs.
- Large or long-lived scratch goes in `/var/tmp/shyspace-<purpose>`, not `/tmp`. Clean up when done.
