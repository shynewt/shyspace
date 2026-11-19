# shyspace

A proof of concept for an end-to-end encrypted team workspace (chat, collaborative pages, AI agents as members) with no server to run. Members sync directly with each other when online, and through any S3-compatible bucket when they are not. The bucket only ever sees ciphertext.

## How it works

| Piece | Library | Role |
|---|---|---|
| Membership and keys | [OpenMLS](https://github.com/openmls/openmls) | The workspace is one MLS group. Each epoch's content key comes from the MLS exporter. |
| Content | XChaCha20-Poly1305, ed25519 | Every change ("op") is signed by its author, then encrypted with the epoch key. |
| Live delivery | [iroh](https://iroh.computer) gossip | Peer-to-peer, with NAT traversal and relay fallback. |
| Durable mailbox | [object_store](https://docs.rs/object_store) | Any S3-compatible bucket: AWS, R2, MinIO, RustFS… |
| Pages | [Yjs](https://yjs.dev) / [yrs](https://github.com/y-crdt/y-crdt) | Markdown in a Yjs text, so humans (CodeMirror) and agents (Rust) edit the same structure. |
| Local state and search | SQLite + FTS5 | Every device keeps a full, searchable copy. |
| Agents | [MCP](https://modelcontextprotocol.io) via [rmcp](https://github.com/modelcontextprotocol/rust-sdk) | `shy mcp` exposes the workspace as tools; the agent is a normal member with its own keys. |
| Desktop app | Tauri 2, React, Motion | |

All state merges without needing a global order: titles and profiles are last-writer-wins by lamport clock, reactions are per-author toggles, pages are CRDTs. That's why ops can arrive in any order, from either transport.

**Membership.** Only the workspace creator (admin) changes membership, which keeps MLS's epoch chain linear without a server. The admin's app accepts invite requests automatically while it's open. Newcomers receive older epoch keys from the admin, so they can read history. Removing someone starts a new epoch whose key they never get.

## Layout

```
crates/shy-core     the workspace engine (crypto, MLS, sync, storage)
crates/shy-cli      `shy`: headless member + MCP server for agents
apps/desktop        Tauri app (React UI in src/, Rust bridge in src-tauri/)
```

## Try it

You need Rust, Node with pnpm, and an S3-compatible bucket. For local testing:

```sh
podman run -d --name shyspace-s3 -p 9000:9000 \
  -e RUSTFS_ACCESS_KEY=shyadmin -e RUSTFS_SECRET_KEY=shysecret123 \
  -v shyspace-s3-data:/data docker.io/rustfs/rustfs:latest
curl --aws-sigv4 "aws:amz:us-east-1:s3" --user shyadmin:shysecret123 -X PUT http://localhost:9000/shyspace
```

**Desktop app** (Linux needs `webkit2gtk4.1-devel gtk3-devel libsoup3-devel openssl-devel`):

```sh
cd apps/desktop && pnpm install
pnpm tauri dev                                   # first member: create a workspace
SHY_DIR=/var/tmp/shy-second pnpm tauri dev       # a second member on the same machine
```

**Add Claude Code as a member:**

```sh
cargo install --path crates/shy-cli
shy join --bot --name "Claude" <invite code>     # the admin's app must be open
claude mcp add shyspace -- shy mcp
```

**Tests:** `cargo test --workspace`. They cover joining, history for newcomers, chat, concurrent page edits, search, removal, restart, and attacks from members and from anyone with bucket access.

## Known limits of this proof of concept

- Anyone holding an invite code can join (the admin's app accepts automatically), and the code contains the bucket credentials.
- A removed member keeps what they already decrypted, and could still forge back-dated ops in epochs they belonged to.
- Anyone with bucket credentials can delete data or disrupt sync (denial of service), but cannot read or forge content.
- The bucket sees metadata: who writes, how much, and when.
- Local data is not encrypted at rest.
- One admin, no message edit/delete, no file attachments, threads, voice, databases or whiteboards yet; no mobile app.
