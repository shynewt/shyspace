#!/usr/bin/env bash
# Runs the UI tests in WebKit inside the official Playwright container, because
# Playwright's WebKit build needs Ubuntu libraries that other distributions lack.
# The image tag follows the installed Playwright, whose browsers it ships.
# The container has its own network, so its dev server on port 1430 doesn't
# clash with one on the host.
set -euo pipefail

desktop_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
playwright_version="$(cd "$desktop_dir" && node -p 'require("./node_modules/@playwright/test/package.json").version')"

# The image has no pnpm, which the config's dev server command uses. Every
# command it runs is already installed in node_modules, so a stand-in that
# runs `pnpm exec <tool>` as `npx <tool>` is enough, and needs no network.
container_command='mkdir -p /tmp/bin \
  && printf "#!/bin/sh\n[ \"\$1\" = exec ] && shift\nexec npx \"\$@\"\n" > /tmp/bin/pnpm \
  && chmod +x /tmp/bin/pnpm \
  && PATH="/tmp/bin:$PATH" exec npx playwright test "$@"'

# keep-id: files the tests write belong to you, not root. label=disable: lets
# the container read the repository under SELinux without relabelling it.
exec podman run --rm --init \
  --userns=keep-id \
  --security-opt label=disable \
  --volume "$desktop_dir:/app" \
  --workdir /app \
  --env HOME=/tmp \
  --env NPM_CONFIG_UPDATE_NOTIFIER=false \
  --env UI_TEST_BROWSERS=webkit \
  "mcr.microsoft.com/playwright:v$playwright_version-noble" \
  bash -c "$container_command" bash "$@"
