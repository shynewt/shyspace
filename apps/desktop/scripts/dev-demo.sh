#!/usr/bin/env bash
# Builds a fresh demo workspace on the local bucket (crates/shy-demo) and opens
# the app in it as its admin. Each run replaces the previous demo. Extra
# arguments go to `tauri dev`.
set -euo pipefail

demo_root="${SHY_DEMO_DIR:-/var/tmp/shyspace-demo}"
echo "Building the demo workspace in $demo_root…" >&2
admin_dir="$(cargo run --quiet -p shy-demo -- "$demo_root")"
SHY_DIR="$admin_dir" exec pnpm tauri dev "$@"
