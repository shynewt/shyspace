# shyspace

An end-to-end encrypted team workspace: chat, collaborative pages, and AI agents as first-class members. There is no server to run. Members sync directly when online, and through any S3-compatible bucket when not. The bucket only ever sees ciphertext.

- `crates/shy-core`: the engine (OpenMLS keys, signed and encrypted ops, iroh gossip, S3 bucket, SQLite)
- `crates/shy-cli`: `shy`, a headless member that serves the workspace to AI agents over MCP
- `apps/desktop`: the desktop app (Tauri + React)

Read [docs/](docs/README.md) for architecture, security, decisions and development. Planned work lives in [issues](https://github.com/shynewt/shyspace/issues).

## Quick start

```sh
cd apps/desktop && pnpm install && pnpm tauri dev
```

Create a workspace with your bucket's details, then create an invite in **Members & invites** and approve the request when it arrives. To add Claude Code as a member:

```sh
cargo install --path crates/shy-cli
shy join --bot --name "Claude" <invite code>   # add --key-file <path> on servers without a keychain
claude mcp add shyspace -- shy mcp
```
