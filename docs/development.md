# Development

## Setup

- Rust (stable), Node with pnpm, Podman or Docker.
- Linux desktop builds need: `webkit2gtk4.1-devel gtk3-devel libsoup3-devel openssl-devel` (Fedora names).
- `cd apps/desktop && pnpm install`

## A local bucket

Any S3-compatible store works. RustFS is easy to run locally:

```sh
podman run -d --name shyspace-s3 -p 9000:9000 \
  -e RUSTFS_ACCESS_KEY=shyadmin -e RUSTFS_SECRET_KEY=shysecret123 \
  -v shyspace-s3-data:/data docker.io/rustfs/rustfs:latest
curl --aws-sigv4 "aws:amz:us-east-1:s3" --user shyadmin:shysecret123 -X PUT http://localhost:9000/shyspace
```

The CLI reads bucket settings from `SHY_S3_ENDPOINT`, `SHY_S3_BUCKET`, `SHY_S3_ACCESS_KEY_ID`, `SHY_S3_SECRET_ACCESS_KEY` (and `SHY_S3_REGION`).

## Running

- **Desktop:** `cd apps/desktop && pnpm tauri dev`. `SHY_DIR=<dir>` picks the data directory, so you can run several members on one machine.
- **CLI:** `cargo run -p shy-cli -- --dir <dir> <create|join|invite|mcp>`. The binary is called `shy`.
- **An agent as a member:** the admin creates an invite (app or `shy invite create`), then `shy join --bot --name Claude <invite>` waits until the admin approves; then `claude mcp add shyspace -- shy mcp`.

Note: one identity can only be online once. Don't run the CLI and the app on the same data directory at the same time.

## Checks

Run all of these before committing:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
(cd apps/desktop && pnpm typecheck)
```

`scripts/e2e_mcp.py` is the end-to-end test: two real `shy` processes, real networking and a real bucket. It needs the local bucket above and `cargo build -p shy-cli`:

```sh
python3 scripts/e2e_mcp.py
```

## Testing the UI

There is no UI test suite yet. To check a change, run the app on a pre-filled workspace and drive a second member through the CLI or MCP so you can watch live updates arrive.

## Conventions

Coding rules, testing and commit conventions are in [AGENTS.md](../AGENTS.md). Clippy enforces some of them (`[workspace.lints]` in `Cargo.toml`).

## Local scratch space

`.local/` is gitignored. Use it for notes, logs and throwaway files that shouldn't be shared.
