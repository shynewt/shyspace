# Development

## Setup

- Rust through rustup, Node with pnpm, Podman or Docker. `rust-toolchain.toml` pins the Rust version, the same for everyone and CI; `rustup toolchain install` in the repository installs it. Bumping it is a deliberate change that fixes any new warnings in the same commit.
- Linux desktop builds need: `webkit2gtk4.1-devel gtk3-devel libsoup3-devel openssl-devel` (Fedora names).
- `cd apps/desktop && pnpm install`
- Huddle audio on Linux also needs `alsa-lib-devel pipewire-devel clang` (Ubuntu: `libasound2-dev libpipewire-0.3-dev clang libclang-dev`). `clang-libs` alone isn't enough: generating the PipeWire bindings needs clang's own headers. Those bindings also need a newer PipeWire than Ubuntu 22.04 has, which is one reason the Linux release builds on 24.04.

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

Segments (`ops/`) are written create-only (If-None-Match: *). To make that a guarantee rather than a habit, give the bucket a policy that refuses writes over an existing object under `ops/` where the provider supports one (not under `mls/`: commits are repaired by plain puts) (AWS S3 bucket policies can require the `s3:if-none-match` condition; check your provider's documentation for the exact form). Setting up a workspace, and testing bucket settings, write one create-only object and refuse a bucket that can't; the bucket probe below also checks that a second write to the same name is refused.

With versioning on, every head rewrite (`heads/`, after each upload) leaves an old version behind. Add a lifecycle rule that expires noncurrent versions and expired delete markers after 1 day for `heads/` (and `probe/`, if you run the bucket probe below); in S3 terms, `NoncurrentVersionExpiration` of 1 day and `ExpiredObjectDeleteMarker` for those prefixes. It's a setup recommendation; the app doesn't change bucket settings.

To check that a real provider does what sync relies on (listings carry an ETag that changes on overwrite, new objects are listed at once, listing after an offset), run the ignored bucket probe with an env file holding the `SHY_S3_*` settings below. It prints nothing from the file and deletes what it writes under `probe/<random>/`:

```sh
SHY_BUCKET_PROBE=/path/to/probe.env cargo test -p shy-core --lib bucket_probe -- --ignored --nocapture
```

The CLI reads bucket settings from `SHY_S3_ENDPOINT`, `SHY_S3_BUCKET`, `SHY_S3_ACCESS_KEY_ID`, `SHY_S3_SECRET_ACCESS_KEY` (and `SHY_S3_REGION`).

## Running

- **Desktop:** `cd apps/desktop && pnpm tauri dev`. In this debug build, Create starts with the local bucket above filled in. The tray icon needs the system's appindicator library on Linux (`libayatana-appindicator-gtk3` on Fedora, `libayatana-appindicator3-1` on Ubuntu); without it the app still runs, but closing the window quits. On macOS the app stays in the Dock when its window is closed (the Dock icon brings it back), with a menu bar item only if turned on in Settings › Notifications. Opening at login (Settings › General) registers whichever binary is running, so in a development build it points at `target/debug`; turn it off again before you leave it. On Linux the entry is `~/.config/autostart/dev.shynewt.shyspace.desktop` (or under `$XDG_CONFIG_HOME`); an AppImage registers its own path (`$APPIMAGE`), and rewrites the entry at each start so a moved AppImage keeps working. Notification permission and the Reply and Mark as read buttons need the installed `.app` (Apple's notification center refuses a process outside a bundle), so `pnpm tauri dev` on macOS shows alerts through the plugin, without them; check them in a built app. The menu bar and monochrome tray glyphs are SVGs in `src-tauri/icons/tray/`, rendered to the raw pixels the app embeds by `render.sh` there. `SHY_DIR=<dir>` makes the app hold just the workspace in that folder, as `shy --dir` does, so you can run several members on one machine (it turns off the one-app-at-a-time rule, which otherwise hands a second launch over to the running app; a development build on Linux keeps a lock of its own, so it runs beside an installed AppImage, and on macOS and Windows takes none); without it, the app holds every workspace in its data folder.
- **CLI:** `cargo run -p shy-cli -- --dir <dir> <create|join|invite|recovery-kit|restore|mcp>`. The binary is called `shy`. It keeps its device key in the system keychain; add `--key-file <path>` (outside the data directory) on machines without one, and in tests. `restore` reads the recovery kit from standard input or `SHY_RECOVERY_KIT`, never from the command line.
- **An agent as a member:** the admin creates an invite (app or `shy invite create`), then `shy join --bot --name Claude` reads the code from standard input (or `SHY_INVITE`, or as a last argument) and waits until the admin approves; then `claude mcp add shyspace -- shy mcp`. Named agents (`--agent <name>`) keep separate members in `~/.local/share/shyspace/agents/<name>`. The agent skill lives in `skills/shyspace/SKILL.md`; `shy skill` prints it, and the MCP server sends the same guidance as its instructions, so keep it in step with the tools.

- **Phone bindings:** `cargo run -p shy-ffi --features bindgen --bin uniffi-bindgen -- generate --library target/debug/libshy_ffi.so --language kotlin --language swift --out-dir <dir>` after `cargo build -p shy-ffi`. The generated code isn't committed; the app builds make it.

Note: one identity can only be online once. Don't run the CLI and the app on the same data directory at the same time.

## The Android app

Everything builds in a container (`apps/android/Dockerfile`: JDK, Android SDK and NDK, Rust with the Android targets, cargo-ndk), so the host needs only Podman or Docker. From `apps/android`:

- `make debug` builds the debug APK (`app/build/outputs/apk/debug/`); `make check` runs the unit tests, lint and the debug build, and compiles the instrumented tests (Kotlin from scratch, as an incremental compile once skipped a test that no longer built); `make unit TESTS='*Channel*'` runs some tests; `make gradle ARGS='…'` runs any Gradle task.
- Every artifact Gradle downloads is checked against its SHA-256 in `gradle/verification-metadata.xml`; after adding or updating a dependency, `make dependency-checksums` writes the file afresh from an empty cache of its own (read its diff; Gradle fetches some metadata only on an empty cache, so the file must never be generated from a warm one). Gradle tries Google's mirror of Maven Central before Maven Central, which rate-limits large downloads; Robolectric's Android runtime comes through Gradle too, so it's cached and checked, and Robolectric runs offline. The base image, the command-line tools and rustup-init are pinned by digest or checksum in the `Dockerfile`, and the vendored AAR by the SHA-256 in `app/build.gradle.kts`.
- Downloads are shared by every worktree in `/var/tmp/shyspace-android-cache`; build output stays in the worktree (`target/android`, `app/build`). Builds sharing the cache take turns (`flock` on `build.lock` there), since Gradle in two containers can't share its cache's locks.
- Debug builds can create a test workspace, filled in with the local bucket as the emulator sees it (`http://10.0.2.2:9000`); on a phone, use the computer's address on the network (the app then asks for local network access, and the computer's firewall must let port 9000 in), or forward the port over USB with `adb reverse tcp:9000 tcp:9000` and use `http://127.0.0.1:9000`. Phones are meant to join as another device of their person, so release builds can't create one.
- The tests run the real core (built for the machine) on an in-memory bucket, through the same generated bindings as the app. There's no emulator in the container: screens are tested with Robolectric. `make screens` renders every screen, light and dark, at a phone's size to `app/build/outputs/roborazzi/` (Roborazzi), to look at a change without a phone.
- `make wrapper` regenerates the Gradle wrapper from a checked distribution; `debug.keystore` is a public key on purpose, so every build installs as an update. Because anyone can sign with it, anyone could install an "update" over a debug build and read its data: never keep a real workspace in a debug build.
- `make device-test` runs the instrumented tests (`app/src/androidTest`: the Keystore, Android's file rules, and HTTPS through the phone's trust store, which needs the internet: `PlatformTlsDeviceTest` contacts `s3.us-east-1.amazonaws.com` with made-up keys, and serves a self-signed certificate on the phone's own `127.0.0.1`) on a phone adb can see: plugged in by USB with USB debugging on, or paired for wireless debugging. It waits up to two minutes for a phone offering adb (unlock it; a locked GrapheneOS phone drops USB data), updates the debug app and leaves it installed, with its data, and removes only the test app afterwards. The adb key is kept in the cache folder, so the phone asks once.

## The iOS app

iOS builds need Apple's SDK, so they run on a Mac (Apple silicon, Xcode 27) or in CI's `ios` job on GitHub's macOS runner; nothing for iOS builds on Linux. From the repository on a Mac with rustup:

- `apps/ios/build-core.sh` builds the Rust core for iPhones and the Apple silicon simulator (`aarch64-apple-ios`, `aarch64-apple-ios-sim`), its Swift bindings, and `ShyCoreFFI.xcframework`, into the local Swift package `apps/ios/ShyCore` (generated, never committed; `release` leaves out the in-memory bucket). Run it again after changing `crates/`.
- `cd apps/ios/ShyCore && xcodebuild test -scheme ShyCore -destination 'platform=iOS Simulator,name=iPhone 17'` runs the Swift tests against the real core.
- Both crates' C declarations live in one module, `ShyCoreFFI` (their `uniffi.toml`); the generated Swift is compiled in the Swift 5 language mode until UniFFI's is Sendable-correct, and everything else in Swift 6.

The phone apps' colours and motion come from `apps/desktop/src/styles.css` through `tools/design-tokens/export.mjs`, which writes `ShyTokens.kt`. `pnpm lint` fails if it's out of date; run `node tools/design-tokens/export.mjs` after changing a colour token.

## A demo workspace

`pnpm dev:demo` (in `apps/desktop`) builds a fresh workspace on the local bucket and opens the app in it as Ada, the admin. It has five more members (one an agent), public channels with threads, reactions, mentions, tables, callouts and code, a private channel, DMs, notes, pages, statuses, a sidebar section, unread messages and two reminders (one due). Building it takes about a minute. Each run replaces the last; it refuses to delete a folder that isn't an earlier demo.

Every member has a data directory in `/var/tmp/shyspace-demo/<name>` (or `$SHY_DEMO_DIR`), with its key in the system keychain, so you can open another one alongside to watch live updates: `SHY_DIR=/var/tmp/shyspace-demo/maya pnpm tauri dev`, or `shy --dir /var/tmp/shyspace-demo/maya mcp`. The tool itself is `crates/shy-demo` (`cargo run -p shy-demo -- <dir>` prints the admin's directory); it uses the `SHY_S3_*` variables when set. All demo messages are written within a few seconds, so they share a time.

## Checks

Run all of these before committing. CI (`.github/workflows/ci.yml`) is the backstop, not the first line: it runs on every push to main, on pull requests, weekly, and from the Actions tab's "Run workflow" button. The real-app job runs only from that button. A newer push cancels a pull request's older run, never main's, so every commit on main gets a verdict. Only main saves the Rust cache, which keeps branches from crowding it out. The jobs name exact runner images (`ubuntu-24.04`, never `ubuntu-latest`), because a moving label changes the build machine without a commit, and for Linux the oldest system the packages run on. Dependabot (`.github/dependabot.yml`) opens one pull request a week that updates the workflows' actions. `.github/workflows/actions.lock` pins every action the workflows use to a commit, so a moved or hijacked tag can't change what runs, and CI's lockfile job (`gh actions-lock --verify`) fails when it's stale. After changing a `uses:` line, run `gh actions-lock` (once: `gh extension install github/gh-actions-lock`) and commit the lockfile with the workflow change; it also narrows a ref like `v7` to the exact release (`v7.0.1`). Do the same on a Dependabot action update if its lockfile check fails. Every job installs Rust from `rust-toolchain.toml` with `rustup toolchain install`, so CI builds with exactly the pinned version.

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

`scripts/upgrade_check.py` checks that this tree opens what the previous release made. It checks out the previous tag (or the one given), builds `tools/upgrade-fixture` there against that release's `shy-core`, and makes a workspace on the local bucket: three members, messages, a reply, an edit, a reaction, a delete, a private channel, a DM, a page with an image, a file, a profile picture, a status and a removal. It records what each member sees. Then it runs shy-core's ignored `tests::upgrade` test, which opens the workspace with this tree, compares what each member sees, syncs, writes, and restores the admin from that release's recovery kit. A difference leaves `actual-<member>.json` beside `expected.json` in the scratch folder (`/var/tmp/shyspace-upgrade-check`, deleted after a run that passes). The fixture is built only by that release, so keep it to what the previous release offers, and keep its snapshot in step with the test's.

```sh
python3 scripts/upgrade_check.py          # against the latest tag before HEAD
python3 scripts/upgrade_check.py v0.1.0
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

Run `scripts/upgrade_check.py` (above) and see it pass first: a new version must open every workspace the last one made. Then push a version tag that matches the version in `apps/desktop/src-tauri/tauri.conf.json` (`git tag v0.1.0 && git push origin v0.1.0`). `.github/workflows/release.yml` runs the CI checks, then builds the app and the `shy` CLI for Linux (`.deb`, `.rpm`, AppImage; built on Ubuntu 24.04, so they need glibc 2.39: Ubuntu 24.04, Debian 13, Fedora 40 or newer), macOS on Apple Silicon (`.dmg`) and Windows (installer), with OpenSSL built in (`--features vendored-openssl`), and attaches them to a draft GitHub release to look over and publish. Each installer is attached a second time under a name without the version (`shyspace-macos-arm64.dmg`, `shyspace-windows-x64-setup.exe`, `shyspace-windows-x64.msi`, `shyspace-linux-x86_64.AppImage`, `.deb`, `.rpm`), so `releases/latest/download/<name>` links always get the newest; the update feed keeps pointing at the versioned files, and the release fails if a copy is missing. Everything else builds them as workflow artifacts only: the Actions tab's "Run workflow" button, a pull request that changes the workflows or `tauri.conf.json`, and a weekly run on main that catches a retired runner image or a broken build before a release needs it. The build machines are `ubuntu-24.04`, `macos-26` and `windows-2025`; GitHub keeps two images per system and retires the oldest, so move them on when the weekly run warns. macOS builds use the hardened runtime, which blocks the microphone unless the app is entitled to it (`src-tauri/Entitlements.plist`; the release workflow checks the app and its update package carry it). A tag's build is signed with a Developer ID and notarized, the ticket stapled: the `release` environment holds `APPLE_CERTIFICATE` (the base64 `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`, set only for tag runs on macOS, and the workflow fails unless Gatekeeper accepts the app. Every other build is signed ad hoc (`bundle.macOS.signingIdentity: "-"` in `tauri.conf.json`), with no notarization, so macOS refuses it at first; the workflow still checks the signature is whole, since a broken one makes macOS call the app damaged. To open one, follow "Building the macOS app by hand" below, from "The app isn't signed".

### Updates

Installed apps update themselves from the latest published release. They look at launch and every 4 hours, download and check an update in the background (`src-tauri/src/updates.rs`), and install it only when the person restarts for it: never during a huddle (also one that starts while the update installs), and only after uploading unsent changes (anything that doesn't make it stays on disk and goes at the next start). Development builds never look. A feed that answers with an error status (404 before the first release is published) means there is nothing to update to and logs nothing; a failed network request, an unreadable feed or a bad signature is logged as a warning. A tag's build checks the tag is the app's version and the public key is set, signs the update packages with the updater key, and attaches `latest.json`; a last job checks `latest.json` offers the version to macOS, Windows, AppImage, `.deb` and `.rpm` installs, each with a signature made for that version. Publish the draft as a normal release, not a pre-release, because apps only look at the latest one. The signature carries the version, and apps refuse a package whose signed version isn't the one `latest.json` offers, so an edited feed can't roll them back to an older build. Every device updates to the same version, and data is kept across updates. On Linux an AppImage updates itself; a `.deb` or `.rpm` install updates too, but asks for an administrator password each time. On macOS the ad-hoc signature changes with every build, so after an update macOS may ask again for the keychain item holding the device key and for the microphone; check this on the first update.

The updater key is set up once, by the admin. Tag no release before its public key is in `plugins.updater.pubkey` in `tauri.conf.json` (an app built without it can't install updates) and before the app shows the "Update ready" prompt, which is what installs one. Back the key file and its password up somewhere safe: without them, no installed app can update again and every device needs a manual reinstall. A key with no password has no second secret to set; GitHub refuses empty secrets.

```sh
# 1. The key pair; it asks for a password. Keep the file safe and out of the repository.
pnpm --dir apps/desktop tauri signer generate -w ~/.tauri/shyspace-updater.key
# 2. A `release` environment that only v* tags may use.
gh api -X PUT repos/shynewt/shyspace/environments/release \
  -F 'deployment_branch_policy[protected_branches]=false' -F 'deployment_branch_policy[custom_branch_policies]=true'
gh api -X POST repos/shynewt/shyspace/environments/release/deployment-branch-policies -f name='v*' -f type=tag
# 3. The private key and its password, as that environment's secrets.
gh secret set TAURI_SIGNING_PRIVATE_KEY --env release < ~/.tauri/shyspace-updater.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --env release
# 4. The public key, one line, for plugins.updater.pubkey.
cat ~/.tauri/shyspace-updater.key.pub
```

## Building the macOS app by hand

The release workflow builds the `.dmg`, but anyone with a Mac (Apple Silicon) can also build it, for example to try a branch. It takes about 15 minutes the first time, mostly installing tools and compiling.

1. Install Apple's command-line tools: `xcode-select --install`.
2. Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`, then open a new terminal.
3. Install Node 22 and pnpm, for example with Homebrew: `brew install node@22 pnpm`.
4. Get the code: `git clone git@github.com:shynewt/shyspace.git && cd shyspace/apps/desktop`.
5. Build: `pnpm install && pnpm tauri build --features vendored-openssl --bundles dmg`. macOS has no OpenSSL to link, so it's built in; that uses the Perl that comes with macOS.
6. The disk image is in `target/release/bundle/dmg/` at the repository root (for example `shyspace_0.1.0_aarch64.dmg`). Share it privately.

The app isn't signed with an Apple identity, so macOS refuses it the first time. Drag it from the disk image to Applications and open it once. If macOS says it's blocked, go to System Settings › Privacy & Security, find "shyspace was blocked", and choose **Open Anyway**. (Since macOS 15, right-click › Open no longer does this.) If it says the app is damaged, or it still won't open, run `xattr -cr /Applications/shyspace.app` once in Terminal, which removes the "downloaded from the internet" mark. After that it opens normally.

## Conventions

Coding rules, testing and commit conventions are in [AGENTS.md](../AGENTS.md). Clippy (`[workspace.lints]` in `Cargo.toml`) and Biome (`apps/desktop/biome.json`; `pnpm format` fixes formatting) enforce some of them. UI conventions are in [design-system.md](design-system.md).

## Local scratch space

`.local/` is gitignored. Use it for notes, logs and throwaway files that shouldn't be shared.
