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

Then turn on versioning, so the bucket keeps old data and Settings › Storage shows "Partly protected" instead of the red warning:

```sh
curl --aws-sigv4 "aws:amz:us-east-1:s3" --user shyadmin:shysecret123 -X PUT "http://localhost:9000/shyspace?versioning" \
  -H 'Content-Type: application/xml' \
  --data '<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Status>Enabled</Status></VersioningConfiguration>'
```

The CLI reads bucket settings from `SHY_S3_ENDPOINT`, `SHY_S3_BUCKET`, `SHY_S3_ACCESS_KEY_ID`, `SHY_S3_SECRET_ACCESS_KEY` (and `SHY_S3_REGION`).

## Running

- **Desktop:** `cd apps/desktop && pnpm tauri dev`. In this debug build, Create starts with the local bucket above filled in. The tray icon needs the system's appindicator library on Linux (`libayatana-appindicator-gtk3` on Fedora, `libayatana-appindicator3-1` on Ubuntu); without it the app still runs, but closing the window quits. `SHY_DIR=<dir>` picks the data directory, so you can run several members on one machine.
- **CLI:** `cargo run -p shy-cli -- --dir <dir> <create|join|invite|recovery-kit|restore|mcp>`. The binary is called `shy`. It keeps its device key in the system keychain; add `--key-file <path>` (outside the data directory) on machines without one, and in tests.
- **An agent as a member:** the admin creates an invite (app or `shy invite create`), then `shy join --bot --name Claude <invite>` waits until the admin approves; then `claude mcp add shyspace -- shy mcp`. Named agents (`--agent <name>`) keep separate members in `~/.local/share/shyspace/agents/<name>`. The agent skill lives in `skills/shyspace/SKILL.md`; `shy skill` prints it, and the MCP server sends the same guidance as its instructions, so keep it in step with the tools.

Note: one identity can only be online once. Don't run the CLI and the app on the same data directory at the same time.

## A demo workspace

`pnpm dev:demo` (in `apps/desktop`) builds a fresh workspace on the local bucket and opens the app in it as Ada, the admin. It has five more members (one an agent), public channels with threads, reactions and mentions, a private channel, DMs, notes, pages, statuses, a sidebar section, unread messages and two reminders (one due). Each run replaces the last; it refuses to delete a folder that isn't an earlier demo.

Every member has a data directory in `/var/tmp/shyspace-demo/<name>` (or `$SHY_DEMO_DIR`), with its key in the system keychain, so you can open another one alongside to watch live updates: `SHY_DIR=/var/tmp/shyspace-demo/maya pnpm tauri dev`, or `shy --dir /var/tmp/shyspace-demo/maya mcp`. The tool itself is `crates/shy-demo` (`cargo run -p shy-demo -- <dir>` prints the admin's directory); it uses the `SHY_S3_*` variables when set. All demo messages are written within a few seconds, so they share a time.

## Checks

Run all of these before committing (CI runs them too, on every push and pull request: `.github/workflows/ci.yml`):

```sh
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
(cd apps/desktop && pnpm typecheck && pnpm lint)
```

`scripts/e2e_mcp.py` is the end-to-end test: two real `shy` processes, real networking and a real bucket. It needs the local bucket above and `cargo build -p shy-cli`:

```sh
python3 scripts/e2e_mcp.py
```

## Testing the UI

`pnpm test:ui` (in `apps/desktop`) runs the Playwright tests in `tests/ui/`. They load the real frontend from `tests/ui/app.html`, which answers every `invoke` from an in-memory fake (`tests/ui/fake/backend.ts`, through Tauri's `mockIPC`) instead of Rust. Tests act as other members through `window.fake` (for example `window.fake.receiveMessage(...)`). The fake copies only the behavior the screens rely on, so keep it in step with `src-tauri/src/lib.rs`. First run: `pnpm exec playwright install chromium-headless-shell`.

The app runs in WebKitGTK, so the tests run in WebKit too. `pnpm test:ui` runs Chromium only, because Playwright's WebKit needs Ubuntu libraries; `UI_TEST_BROWSERS` (`chromium`, `webkit` or both, comma-separated) picks others, and CI runs both. `pnpm test:ui:webkit` (`scripts/test-ui-webkit.sh`) runs the WebKit tests in the official Playwright container with Podman, so it works on any distribution; it takes the same arguments as `playwright test`. The container has its own network, so its dev server doesn't clash with one on the host's port 1430. Failures there are usually real WebKit differences: fix the app, not the test.

`pnpm test:app` drives the real built app (Rust core, real bucket) through `tauri-driver`, with the `shy` CLI as a second member (`tests/app/`). It needs WebKit's WebDriver for webkit2gtk 4.1 (Ubuntu's `webkit2gtk-driver`; Fedora doesn't package it), a display (`xvfb-run`), an unlocked keyring (`printf pass | gnome-keyring-daemon --unlock`), and the local bucket. CI runs it on Ubuntu; locally, run it in an Ubuntu container with the same packages, with `SHY_APP` and `SHY_CLI` pointing at the built binaries.

To see every shared component, open the app with `#gallery` in development. To check a change by hand, run the app on a pre-filled workspace and drive a second member through the CLI or MCP so you can watch live updates arrive.

## Conventions

Coding rules, testing and commit conventions are in [AGENTS.md](../AGENTS.md). Clippy (`[workspace.lints]` in `Cargo.toml`) and Biome (`apps/desktop/biome.json`; `pnpm format` fixes formatting) enforce some of them. UI conventions are in [design-system.md](design-system.md).

## Local scratch space

`.local/` is gitignored. Use it for notes, logs and throwaway files that shouldn't be shared.
