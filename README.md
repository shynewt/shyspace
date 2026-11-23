<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/brand/logo-dark.svg">
    <img src="docs/brand/logo-light.svg" alt="shyspace" width="300">
  </picture>
</p>

<p align="center"><b>Chat, pages and AI agents. End-to-end encrypted. No server to run.</b></p>

An end-to-end encrypted team workspace: chat, collaborative pages, and AI agents as first-class members. There is no server to run. Members sync directly when online, and through any S3-compatible bucket when not. The bucket only ever sees ciphertext.

- `crates/shy-core`: the engine (OpenMLS keys, signed and encrypted ops, iroh gossip, S3 bucket, SQLite)
- `crates/shy-cli`: `shy`, a headless member that serves the workspace to AI agents over MCP
- `crates/shy-demo`: builds a demo workspace for trying the app by hand
- `apps/desktop`: the desktop app (Tauri + React)

Read [docs/](docs/README.md) for architecture, security, decisions and development. Planned work lives in [issues](https://github.com/shynewt/shyspace/issues).

## Quick start

```sh
cd apps/desktop && pnpm install && pnpm tauri dev
```

Create a workspace with your bucket's details, then choose **Invite people** in the menu under the workspace's name. Approve the request when it arrives (the menu shows it too). To add Claude Code as a member (Settings › Agents shows the same commands):

```sh
cargo install --path crates/shy-cli
shy join --bot --name "Claude"   # paste the invite code when asked; add --key-file <path> on servers without a keychain
claude mcp add shyspace -- shy mcp
mkdir -p ~/.claude/skills/shyspace && shy skill > ~/.claude/skills/shyspace/SKILL.md
```

The code is read from standard input (or `SHY_INVITE`) so it stays out of shell history; it can also go last on the command line.

Each agent should be its own member, so people can tell them apart and remove one without the others. Give each a name with `--agent` (a data folder opens in one process at a time, so two sessions running at once need two agents):

```sh
shy --agent reviewer join --bot --name "Reviewer"
claude mcp add shyspace-reviewer -- shy --agent reviewer mcp
shy agents   # lists the agents on this machine
```
