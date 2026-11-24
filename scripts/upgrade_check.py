#!/usr/bin/env python3
"""Upgrade check: does this version open what the previous release made?

Builds the previous release's `tools/upgrade-fixture` against that release's
shy-core, makes a workspace with it on the local bucket, then runs shy-core's
ignored `tests::upgrade` test, which opens it with this tree and compares what
each member sees, syncs and writes, and restores the admin from the kit.

Usage: scripts/upgrade_check.py [TAG]   (default: the latest tag before HEAD)

Needs the local bucket (see docs/development.md). Scratch goes in
UPGRADE_DIR (default /var/tmp/shyspace-upgrade-check); it is deleted after a
run that passes and kept after one that fails, for looking into.
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORK_DIR = Path(os.environ.get("UPGRADE_DIR", "/var/tmp/shyspace-upgrade-check"))
FIXTURE = "tools/upgrade-fixture"
TEST = "tests::upgrade::the_previous_release_s_workspace_opens_unchanged"


def run(*command: str, cwd: Path = ROOT, environment: dict | None = None) -> str:
    print("$", " ".join(command), flush=True)
    finished = subprocess.run(
        ["nice", "-n", "19", *command], cwd=cwd, env={**os.environ, **(environment or {})},
        capture_output=True, text=True,
    )
    if finished.returncode != 0:
        raise RuntimeError(f"{' '.join(command)} failed:\n{finished.stdout[-4000:]}\n{finished.stderr[-4000:]}")
    return finished.stdout


def previous_tag() -> str:
    return run("git", "describe", "--tags", "--abbrev=0", "HEAD^").strip()


def check_out(tag: str, old: Path) -> None:
    """A checkout of `tag` with this tree's fixture added to its workspace."""
    if old.exists():
        run("git", "worktree", "remove", "--force", str(old))
    run("git", "worktree", "add", "--detach", str(old), tag)
    shutil.copytree(ROOT / FIXTURE, old / FIXTURE)
    manifest = old / "Cargo.toml"
    text = manifest.read_text()
    if "members = [" not in text:
        raise RuntimeError(f"{tag}'s Cargo.toml has no members list to add the fixture to")
    manifest.write_text(text.replace("members = [", f'members = ["{FIXTURE}", ', 1))


def main() -> int:
    tag = sys.argv[1] if len(sys.argv) > 1 else previous_tag()
    old = WORK_DIR / "old"
    workspace = WORK_DIR / "workspace"
    WORK_DIR.mkdir(parents=True, exist_ok=True)
    shutil.rmtree(workspace, ignore_errors=True)
    try:
        check_out(tag, old)
        run("cargo", "build", "-j", "6", "-p", "upgrade-fixture", cwd=old)
        print(run(str(old / "target/debug/upgrade-fixture"), str(workspace)).strip())
        run("cargo", "test", "-j", "6", "-p", "shy-core", "--lib", TEST, "--", "--ignored", "--exact",
            environment={"SHY_UPGRADE_DIR": str(workspace)})
    except RuntimeError as error:
        print(f"upgrade check from {tag} failed; scratch kept in {WORK_DIR}\n{error}", file=sys.stderr)
        return 1
    run("git", "worktree", "remove", "--force", str(old))
    shutil.rmtree(WORK_DIR)
    print(f"this tree opens what {tag} made: upgrade check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
