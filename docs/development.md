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

Segments (`ops/`) are written create-only (If-None-Match: *). To make that a guarantee rather than a habit, give the bucket a policy that refuses writes over an existing object under `ops/` where the provider supports one (not under `mls/`: commits are repaired by plain puts) (AWS S3 bucket policies can require the `s3:if-none-match` condition; check your provider's documentation for the exact form). The bucket probe below checks that create-only writes are honoured.

With versioning on, every head rewrite (`heads/`, after each upload) leaves an old version behind. Add a lifecycle rule that expires noncurrent versions and expired delete markers after 1 day for `heads/` (and `probe/`, if you run the bucket probe below); in S3 terms, `NoncurrentVersionExpiration` of 1 day and `ExpiredObjectDeleteMarker` for those prefixes. It's a setup recommendation; the app doesn't change bucket settings.

To check that a real provider does what sync relies on (listings carry an ETag that changes on overwrite, new objects are listed at once, listing after an offset), run the ignored bucket probe with an env file holding the `SHY_S3_*` settings below. It prints nothing from the file and deletes what it writes under `probe/<random>/`:

```sh
SHY_BUCKET_PROBE=/path/to/probe.env cargo test -p shy-core --lib bucket_probe -- --ignored --nocapture
```

The CLI reads bucket settings from `SHY_S3_ENDPOINT`, `SHY_S3_BUCKET`, `SHY_S3_ACCESS_KEY_ID`, `SHY_S3_SECRET_ACCESS_KEY` (and `SHY_S3_REGION`).

## Running

- **Desktop:** `cd apps/desktop && pnpm tauri dev`. In this debug build, Create starts with the local bucket above filled in. The tray icon needs the system's appindicator library on Linux (`libayatana-appindicator-gtk3` on Fedora, `libayatana-appindicator3-1` on Ubuntu); without it the app still runs, but closing the window quits. `SHY_DIR=<dir>` picks the data directory, so you can run several members on one machine.
- **CLI:** `cargo run -p shy-cli -- --dir <dir> <create|join|invite|recovery-kit|restore|mcp>`. The binary is called `shy`. It keeps its device key in the system keychain; add `--key-file <path>` (outside the data directory) on machines without one, and in tests. `restore` reads the recovery kit from standard input or `SHY_RECOVERY_KIT`, never from the command line.
- **An agent as a member:** the admin creates an invite (app or `shy invite create`), then `shy join --bot --name Claude` reads the code from standard input (or `SHY_INVITE`, or as a last argument) and waits until the admin approves; then `claude mcp add shyspace -- shy mcp`. Named agents (`--agent <name>`) keep separate members in `~/.local/share/shyspace/agents/<name>`. The agent skill lives in `skills/shyspace/SKILL.md`; `shy skill` prints it, and the MCP server sends the same guidance as its instructions, so keep it in step with the tools.

Note: one identity can only be online once. Don't run the CLI and the app on the same data directory at the same time.

## A demo workspace

`pnpm dev:demo` (in `apps/desktop`) builds a fresh workspace on the local bucket and opens the app in it as Ada, the admin. It has five more members (one an agent), public channels with threads, reactions, mentions, tables, callouts and code, a private channel, DMs, notes, pages, statuses, a sidebar section, unread messages and two reminders (one due). Building it takes about a minute. Each run replaces the last; it refuses to delete a folder that isn't an earlier demo.

Every member has a data directory in `/var/tmp/shyspace-demo/<name>` (or `$SHY_DEMO_DIR`), with its key in the system keychain, so you can open another one alongside to watch live updates: `SHY_DIR=/var/tmp/shyspace-demo/maya pnpm tauri dev`, or `shy --dir /var/tmp/shyspace-demo/maya mcp`. The tool itself is `crates/shy-demo` (`cargo run -p shy-demo -- <dir>` prints the admin's directory); it uses the `SHY_S3_*` variables when set. All demo messages are written within a few seconds, so they share a time.

## Checks

Run all of these before committing. CI (`.github/workflows/ci.yml`) is the backstop, not the first line: it runs on every push to main, on pull requests, weekly, and from the Actions tab's "Run workflow" button. The real-app job runs only from that button. A newer push cancels a pull request's older run, never main's, so every commit on main gets a verdict. Only main saves the Rust cache, which keeps branches from crowding it out. The jobs name exact runner images (`ubuntu-24.04`, never `ubuntu-latest`), because a moving label changes the build machine without a commit, and for Linux the oldest system the packages run on. Dependabot (`.github/dependabot.yml`) opens one pull request a week that updates the workflows' actions.

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

## Capacity probes

`crates/shy-core/src/tests/capacity/` builds large workspaces (years of messages, a newcomer catching up, hours of typing in pages next to chat, many members, many files and one at the size limit) and prints how time, disk and bucket requests grow. The bucket is in memory, counts every request and can add a delay to each, as a bucket over the internet would. The probes are slow, so `cargo test` skips them; run one at a time, with temporary files on disk rather than in `/tmp` (RAM on many machines):

```sh
TMPDIR=/var/tmp/shyspace-capacity nice cargo test --release -p shy-core \
    capacity::history -- --ignored --nocapture --test-threads=1
```

`SHY_CAPACITY_SCALE` multiplies every size (`0.1` for a quick run). They check that results are right at each size, but don't fail on timings, which depend on the machine.

## Testing the UI

`pnpm test:ui` (in `apps/desktop`) runs the Playwright tests in `tests/ui/`. They load the real frontend from `tests/ui/app.html`, built first and served as one bundle (`vite.ui-tests.config.ts`; the dev server's per-module loading was slower than the tests' checks on CI), which answers every `invoke` from an in-memory fake (`tests/ui/fake/backend.ts`, through Tauri's `mockIPC`) instead of Rust. Tests act as other members through `window.fake` (for example `window.fake.receiveMessage(...)`). The fake copies only the behavior the screens rely on, so keep it in step with `src-tauri/src/lib.rs`. First run: `pnpm exec playwright install chromium-headless-shell`.

The app runs in WebKitGTK, so the tests run in WebKit too. `pnpm test:ui` runs Chromium only, because Playwright's WebKit needs Ubuntu libraries; `UI_TEST_BROWSERS` (`chromium`, `webkit` or both, comma-separated) picks others, and CI runs WebKit only, the engine the app ships. Playwright runs 4 browsers at once locally and 3 on CI (`playwright.config.ts`), so a full run doesn't take over a laptop. `pnpm test:ui:webkit` (`scripts/test-ui-webkit.sh`) runs the WebKit tests in the official Playwright container with Podman, so it works on any distribution; it takes the same arguments as `playwright test`. The container has its own network, so its test server doesn't clash with one on the host's port 1430. Failures there are usually real WebKit differences: fix the app, not the test.

`pnpm test:app` drives the real built app (Rust core, real bucket) through `tauri-driver`, with the `shy` CLI as a second member (`tests/app/`). It needs WebKit's WebDriver for webkit2gtk 4.1 (Ubuntu's `webkit2gtk-driver`; Fedora doesn't package it), a display (`xvfb-run`), an unlocked keyring (`printf pass | gnome-keyring-daemon --unlock`), and the local bucket. CI runs it on Ubuntu when started by hand; locally, run it in an Ubuntu container with the same packages, with `SHY_APP` and `SHY_CLI` pointing at the built binaries.

To see every shared component, open the app with `#gallery` in development. To check a change by hand, run the app on a pre-filled workspace and drive a second member through the CLI or MCP so you can watch live updates arrive.

## Releasing

Push a version tag that matches the version in `apps/desktop/src-tauri/tauri.conf.json` (`git tag v0.1.0 && git push origin v0.1.0`). `.github/workflows/release.yml` runs the CI checks, then builds the app and the `shy` CLI for Linux (`.deb`, `.rpm`, AppImage; built on Ubuntu 24.04, so they need glibc 2.39: Ubuntu 24.04, Debian 13, Fedora 40 or newer), macOS on Apple Silicon (`.dmg`) and Windows (installer), with OpenSSL built in (`--features vendored-openssl`), and attaches them to a draft, pre-release GitHub release to look over and publish. Everything else builds them as workflow artifacts only: the Actions tab's "Run workflow" button, a pull request that changes the workflows or `tauri.conf.json`, and a weekly run on main that catches a retired runner image or a broken build before a release needs it. The build machines are `ubuntu-24.04`, `macos-26` and `windows-2025`; GitHub keeps two images per system and retires the oldest, so move them on when the weekly run warns. macOS builds aren't signed yet, so testers allow the app once in System Settings › Privacy & Security › Open Anyway.

## Building the macOS app by hand

The release workflow builds the `.dmg`, but a tester with a Mac (Apple Silicon) can also build it, for example to try a branch. It takes about 15 minutes the first time, mostly installing tools and compiling.

1. Install Apple's command-line tools: `xcode-select --install`.
2. Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`, then open a new terminal.
3. Install Node 22 and pnpm, for example with Homebrew: `brew install node@22 pnpm`.
4. Get the code: `git clone git@github.com:shynewt/shyspace.git && cd shyspace/apps/desktop`.
5. Build: `pnpm install && pnpm tauri build --features vendored-openssl --bundles dmg`. macOS has no OpenSSL to link, so it's built in; that uses the Perl that comes with macOS.
6. The disk image is in `target/release/bundle/dmg/` at the repository root (for example `shyspace_0.1.0_aarch64.dmg`). Share it privately.

The app isn't signed, so macOS refuses it the first time. Open it once, then go to System Settings › Privacy & Security, find "shyspace was blocked", and choose **Open Anyway**. (Since macOS 15, right-click › Open no longer does this.) After that it opens normally.

## Conventions

Coding rules, testing and commit conventions are in [AGENTS.md](../AGENTS.md). Clippy (`[workspace.lints]` in `Cargo.toml`) and Biome (`apps/desktop/biome.json`; `pnpm format` fixes formatting) enforce some of them. UI conventions are in [design-system.md](design-system.md).

## Local scratch space

`.local/` is gitignored. Use it for notes, logs and throwaway files that shouldn't be shared.
