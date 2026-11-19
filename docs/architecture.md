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
- **`apps/desktop`**: the desktop app. `src-tauri/src/lib.rs` maps each UI command to one `Workspace` call and forwards core changes to the UI as `change` events. The UI is React in `src/`.

## Core ideas

1. **One workspace = one MLS group.** OpenMLS handles membership and key agreement. MLS is only used to agree on one secret per *epoch* (a period between membership changes). Content is encrypted with a key derived from that secret.
2. **Everything is an op.** Any change (message, reaction, page edit, rename…) is an `Op` (`ops.rs`). The author signs it with their identity key, then it is encrypted with the epoch key (`crypto.rs`). The result is an `Envelope`.
3. **Two ways to deliver, same envelopes.** Envelopes are sent live over iroh gossip and also uploaded to the bucket. A member that was offline catches up from the bucket.
4. **No ordering needed.** Every op merges the same way whatever order it arrives in. That is why two unordered transports are enough:
   - names, titles and profiles: last writer wins, by lamport clock;
   - reactions: per-author on/off toggles;
   - page content: Yjs CRDT updates;
   - creations (channel, message, page): the id contains the author's key prefix, so only the author can create or overwrite it.
5. **Local first.** Each device stores ops and the views built from them in SQLite (`store.rs`). Reads never touch the network. Search uses SQLite FTS5, on the device.

## Identity

Each member has an iroh ed25519 key (`identity.key` in their data directory). It is their network address, signs their ops, and is their MLS credential identity. Their MLS signature key is a separate key, bound to the identity by a signature (`mls::binding_message`).

## Membership

Only the admin (the workspace creator) changes membership. This keeps a single, linear chain of epochs without needing a server to order commits.

- **Joining:** an invite code (`invite.rs`) carries the workspace id, bucket settings, admin key, admin MLS key and a token. The joiner uploads a key package with their identity binding to `join/<token>/<joiner>`. The admin's app accepts automatically when online: it signs and saves the commit, applies it, then uploads it with a Welcome and the *key history* (all older epoch keys, their members, and frozen logs of removed members), so the newcomer can read history.
- **Removing:** the admin pulls the member's latest ops, then commits the removal with their last sequence number. The new epoch's key never reaches them, and later ops from them are rejected.
- **Commits** are wrapped in the admin's signature (`SignedCommit`). Members ignore anything else in a commit slot. The admin re-uploads its commits if they go missing or are altered.

## Sync loop (`sync.rs`)

Runs every 8 s, and 600 ms after any local write or nudge. Each step runs even if another fails:

1. apply new admin commits (advance epochs);
2. retry parked ops;
3. admin only: repair commits, accept join requests, deliver Welcomes;
4. pull new segments from everyone who was ever a member (newcomers need removed members' history too);
5. push our unpushed ops as one segment;
6. refresh the gossip allowlist to current members, then connect to members listed in the bucket.

An incoming op is applied only if it decrypts, its signature is valid, and its author was a member in that op's epoch. Ops we can't apply yet (unknown epoch, clock far ahead) are parked in `pending_ops` and retried.

## Bucket layout

Everything is under a `<workspace id>/` prefix. Only ciphertext and public ids are stored.

| Path | Content |
|---|---|
| `ops/<author>/<first seq>-<last seq>` | a segment of that author's envelopes |
| `mls/commit/<from epoch>` | admin-signed MLS commits |
| `mls/keyring/<epoch>` | key history for newcomers, MLS-encrypted |
| `join/<token>/<joiner>` | join requests |
| `welcome/<joiner>` | the admin's answer |
| `peers/<id>` | gossip bootstrap hints |

## Pages

A page is markdown in one Yjs text named `content` (`docs.rs`). The desktop editor (CodeMirror + `y-codemirror.next`) and agents (Rust `yrs`) edit the same structure. Agent edits change only the part that differs, so they don't clash with people typing nearby. A page shared in chat (`shy://page/<id>`) shows as a live card.

## Files in shy-core

| Path | Job |
|---|---|
| `workspace/` | the public API: `lifecycle` (create, join, open), `actions` (writes), `reads` |
| `membership/` | `mls` (OpenMLS wrapper), `invite`, `commit` (admin-signed commits), `keyring` (history for newcomers), `admin` (accepting and removing members), `follow` (members applying commits) |
| `sync/` | the background loop, `receive` (verifying and applying others' ops), `replicate` (bucket pull and push, gossip peers) |
| `storage/` | SQLite: `schema` (migrations), `oplog`, `epochs`, `apply` (folding ops into views), `queries` |
| `transport/` | `bucket` (S3 layout and access), `gossip` (iroh) |
| `crypto.rs` | sign-then-encrypt ops |
| `ops.rs` | op types, owned ids, timestamps |
| `pages.rs` | Yjs page helpers |
| `views.rs` | read models sent to the UI and agents |
| `config.rs`, `identity.rs` | files in a member's data directory |
| `tests/` | multi-member scenarios (`collaboration`) and attacks (`hardening`) |

## Database migrations

The schema is a list of migrations in `storage/schema.rs`, applied in order and tracked by SQLite's `user_version`. Never edit a released migration; append a new one. A database from a newer version is refused rather than misread.
