# Architecture

shyspace is a team workspace (chat, pages, AI agents) where every device keeps a full, encrypted-in-transit copy of the data, and no server can read anything.

## The big picture

```
 Desktop app (Tauri + React)        shy CLI (MCP server for agents)
            │                                   │
            └───────────── shy-core ────────────┘
                   │            │           │
              SQLite (local)  iroh gossip  S3 bucket
                              (live, P2P)  (mailbox for offline members)
```

- **`crates/shy-core`**: the whole engine. Every client (desktop app, CLI, agents) is a thin layer on top of it.
- **`crates/shy-cli`**: the `shy` command. It runs a member without a UI and serves the workspace to AI agents over MCP.
- **`crates/shy-demo`**: a development tool that fills a fresh workspace with demo people and content, through the same `Workspace` API.
- **`apps/desktop`**: the desktop app. `src-tauri/src/lib.rs` maps each UI command to one `Workspace` call and forwards core changes to the UI as `change` events. The UI is React in `src/`.

## Core ideas

1. **One workspace = one MLS group.** OpenMLS handles membership and key agreement. MLS is only used to agree on one secret per *epoch* (a period between membership changes). Content is encrypted with a key derived from that secret.
2. **Everything is an op.** Any change (message, reaction, page edit, rename…) is an `Op` (`ops.rs`). The author signs it with their identity key, then it is encrypted with the epoch key (`crypto.rs`). The result is an `Envelope`.
3. **Two ways to deliver, same envelopes.** Envelopes are sent live over iroh gossip and also uploaded to the bucket. A member that was offline catches up from the bucket. Gossip also carries signed presence heartbeats (every 25 s, and when a peer connects), which are never stored.
4. **No ordering needed.** Every op merges the same way whatever order it arrives in. That is why two unordered transports are enough:
   - names, titles, channel renames, message edits, profiles, profile details and statuses: last writer wins, by lamport clock (edits are kept apart from what they change, so an edit may arrive first);
   - deletes: permanent markers that win over everything else about their target; later ops touching a deleted item are dropped;
   - reactions: per-author on/off toggles;
   - page content: Yjs CRDT updates;
   - creations (channel, message, page): the id contains the author's key prefix, so only the author can create or overwrite it.
5. **Local first.** Each device stores ops and the views built from them in SQLite (`store.rs`). Reads never touch the network. Search uses SQLite FTS5, on the device, with filters parsed in `search.rs` (`from:`, `in:`, `before:`, `after:`).
6. **Mentions are links in the text** (`mentions.rs`): `[@Ada](shy://member/<id>)`, `[@channel](shy://mention/channel)`, `[@here](shy://mention/here)`. They need no op of their own; each device works out what mentions it, so this works under end-to-end encryption.
7. **Threads are replies** (`storage/threads.rs`): a message with `reply_to` lives in the thread of the message it eventually answers, in the same channel, worked out when reading so arrival order doesn't matter. A reply whose parent is missing shows in the channel. A thread's first message carries a summary of its replies: the count, the latest few distinct repliers, and the last reply's author with a short one-line preview cut in the core. You follow threads you started, replied in or were mentioned in, unless you choose otherwise; their new replies alert you.
8. **Alerts are decided on the device** (`sync/alerts.rs`): a new message from someone else raises `Change::Alert` when that channel's level (all, mentions, or nothing) allows it and it was written in the last 10 minutes, so catching up never floods you. The desktop app shows it unless you're looking at that channel, titled with the person for a DM ("Grace"), the group ("Grace in your group with Alan") or the channel ("Grace in #plans"), with the sender's avatar as its picture where the system shows one (clicking it brings the window forward at that message; see `src-tauri/src/alerts.rs`), and keeps running in the tray when its window is closed. How many things need you (Inbox items, unread DMs you haven't muted, due reminders) shows in the window title, as a taskbar or dock badge where the system has one, and beside the tray icon.
9. **The inbox** (`workspace/inbox.rs`) gathers unread mentions and new replies in followed threads across channels, from the same read positions, so reading something anywhere clears it.
10. **Personal state stays on the device.** Things only you care about, like where you've read up to in each channel or thread, alert levels, which threads you follow, unsent drafts, what you saved for later (with reminders) and how you group channels into sidebar sections (`storage/reading.rs`), are plain local rows, not ops: nobody else sees them, and they don't sync between devices (one member is one device today, #53).

## Identity and local data

Each member has an iroh ed25519 key. It is their network address, signs their ops, and is their MLS credential identity. Their MLS signature key is a separate key, bound to the identity by a signature (`mls::binding_message`).

A member's data directory holds two SQLCipher-encrypted databases (`shy.db`, `mls.db`) and a `workspace.json` with only non-secret settings (workspace id, name, admin). The identity key and bucket credentials live inside `shy.db`. Both databases are encrypted with keys derived from a random *device key* (`keys.rs`), kept outside the directory: in the OS keychain, or in a key file (`shy --key-file`) on servers.

## Membership

Only the admin (the workspace creator) changes membership. This keeps a single, linear chain of epochs without needing a server to order commits.

- **Inviting:** the admin creates an invite (`membership/invite.rs`). Its code is single-use, expires after 7 days, and holds the workspace id, the admin's keys, a token and two presigned links: one to upload a join request, one to download the answer. It contains no bucket credentials.
- **Joining:** the joiner posts their key package, identity binding and name through the request link. The admin's app shows the request; the admin approves or declines. On approval it signs and saves the commit, applies it, and answers with the MLS Welcome plus an encrypted *grant*: the key history (older epoch keys, their members, removed members' frozen logs) and the bucket credentials. The answer is re-sent until the newcomer posts, then deleted. The joiner can cancel while waiting (`JoinCancel`); a cancel only counts before the answer arrives, so a join is never left half done, and the request stays in the admin's list until declined or expired. Asking again from the same device keeps that first request (and its name); its key package's secret is still on the device, so approving it lets the device in.
- **Recovery:** the admin can export a *recovery kit* (a code holding a random recovery key and the bucket settings). After every membership change, the admin's app uploads an encrypted snapshot of its admin state (MLS state, identity, keys, members, commits, invites; not content) to `recovery/snapshot`. `restore` on a new device decrypts it, then re-downloads everything else from the bucket, including the admin's own log so its sequence numbers continue. See `membership/recovery.rs`.
- **Storage settings:** the admin can change the bucket's region and keys (`workspace/storage.rs`). The change is an op like any other, so it reaches members live or through the bucket; each member stores it and reconnects. The endpoint and bucket can't change: the data would stay behind.
- **Removing:** the admin pulls the member's latest ops, then commits the removal with their last sequence number. The new epoch's key never reaches them, and later ops from them are rejected.
- **Commits** are wrapped in the admin's signature (`SignedCommit`). Members ignore anything else in a commit slot. The admin re-uploads its commits if they go missing or are altered.

## Sync loop (`sync.rs`)

Runs every 8 s, and 600 ms after any local write or nudge. Each step runs even if another fails:

1. apply new admin commits (advance epochs);
2. retry parked ops;
3. repair missing or altered commits in the bucket (everyone); admin only: refresh the recovery snapshot first, then collect join requests and deliver answers;
4. pull new segments from everyone who was ever a member, ourselves included (newcomers need removed members' history; a restored device needs its own);
5. learn the forks proved in the bucket, and put there the proofs we have that it lacks (`sync/forks.rs`);
6. push our unpushed ops as segments, each under the 32 MB that readers accept;
7. refresh the gossip allowlist to current members, then connect to members listed in the bucket.

An incoming op is applied only if it decrypts, its signature is valid, its author was a member in that op's epoch, and it directly follows that author's previous op (logs have no gaps). Ops we can't apply yet (unknown epoch, clock far ahead) are parked in `pending_ops` and retried. If a second, different version of an op we already have arrives, signed by its author, neither version counts (see Forked logs in `security.md`).

## Bucket layout

Everything is under a `<workspace id>/` prefix. Only ciphertext and public ids are stored.

| Path | Content |
|---|---|
| `ops/<author>/<first seq>-<last seq>` | a segment of that author's envelopes |
| `ops/forks/<author>/<seq>/<hash>` | proof that the author signed two versions of that op: both envelopes |
| `mls/commit/<from epoch>` | admin-signed MLS commits |
| `invites/<token>/request` | a joiner's request, written through a presigned link |
| `invites/<token>/answer` | the admin's answer, read through a presigned link |
| `recovery/snapshot` | the admin's recovery snapshot, encrypted with the recovery key |
| `peers/<id>` | gossip bootstrap hints, rewritten when a member starts and every 6 hours; members silent for a day aren't dialed, and nobody is redialed more often than every 10 minutes unless they announce again (`sync/peers.rs`) |
| `health/versioning-probe`, `ops/lock-probe-<member>` | small probes that prove the bucket keeps deleted data (see `workspace/protection.rs`) |

## Pages

A page is markdown in one Yjs text named `content` (`docs.rs`). The desktop editor (CodeMirror + `y-codemirror.next`) and agents (Rust `yrs`) edit the same structure. Agent edits change only the part that differs, so they don't clash with people typing nearby. A page shared in chat (`shy://page/<id>`) shows as a live card. The editor shows the markdown as live preview (`features/pages/editor/`, rules in `design-system.md`); what is stored stays plain markdown.

## Files in shy-core

| Path | Job |
|---|---|
| `workspace/` | the public API: `lifecycle` (create, join, open), `actions` (writes), `reads`, `presence` (heartbeats and status), `protection` (whether the bucket keeps deleted data) |
| `membership/` | `mls` (OpenMLS wrapper), `invite`, `commit` (admin-signed commits), `keyring` (history for newcomers), `admin` (invites, approvals, removals), `follow` (members applying commits), `recovery` (admin recovery kit and snapshots) |
| `sync/` | the background loop, `receive` (verifying and applying others' ops), `forks` (two versions of one op), `replicate` (bucket pull and push, gossip peers) |
| `storage/` | SQLite: `schema` (migrations), `oplog`, `epochs`, `forks`, `apply` (folding ops into views), `revisions` (edits, renames, deletes), `profile_details`, `rebuild` (undoing ops that turned out not to count), `private`, `private_members` and `sealed` (private channel state), `sealing`, `queries` |
| `transport/` | `bucket` (S3 layout and access), `gossip` (iroh) |
| `private/` | private channels and DMs: `ids`, `ops` (what's inside sealed ops and grants), `crypto` (sealed content, key wraps, commitments), `sealing` (each member's key for receiving channel keys), `apply` (grants and waiting sealed ops), `rules` (which private ops count), `actions` (create, add, remove, leave, hand over, DMs), `reads` |
| `crypto.rs` | sign-then-encrypt ops |
| `ops.rs` | op types, owned ids, timestamps |
| `profile.rs` | profile details (role, contact, time zone, working hours) and how others' are cleaned |
| `pages.rs` | Yjs page helpers |
| `views.rs` | read models sent to the UI and agents |
| `keys.rs` | the device key and where it's kept (keychain or file) |
| `config.rs`, `identity.rs` | workspace settings (including the current bucket settings) and the identity key |
| `tests/` | multi-member scenarios (`collaboration`, `private`) and attacks (`hardening`, `private_attacks`) |

## Database migrations

The schema is a list of migrations in `storage/schema.rs`, applied in order and tracked by SQLite's `user_version`. Never edit a released migration; append a new one. A database from a newer version is refused rather than misread.
