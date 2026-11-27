#!/usr/bin/env python3
"""End-to-end test: two real `shy` processes, real networking, a real bucket.

An admin and a bot member talk over MCP. Checks joining, live delivery over
gossip, page creation and search across members, then the admin linking a
laptop of theirs by code and removing it.

Needs a running S3-compatible bucket (see docs/development.md) and a built CLI
(`cargo build -p shy-cli`). Settings come from the environment:
  SHY_S3_ENDPOINT, SHY_S3_BUCKET, SHY_S3_ACCESS_KEY_ID, SHY_S3_SECRET_ACCESS_KEY
  SHY_BIN   path to the shy binary (default: target/debug/shy)
  E2E_DIR   working directory (default: .local/e2e, wiped on each run)
"""

import json
import os
import shutil
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SHY = os.environ.get("SHY_BIN", str(ROOT / "target/debug/shy"))
WORK_DIR = Path(os.environ.get("E2E_DIR", ROOT / ".local/e2e"))
LOCAL_BUCKET = {
    "SHY_S3_ENDPOINT": "http://localhost:9000",
    "SHY_S3_BUCKET": "shyspace",
    "SHY_S3_ACCESS_KEY_ID": "shyadmin",
    "SHY_S3_SECRET_ACCESS_KEY": "shysecret123",
}
GOSSIP_SETTLE_SECONDS = 6
DELIVERY_TIMEOUT_SECONDS = 30


class McpMember:
    """One `shy mcp` process, driven over JSON-RPC on stdin/stdout."""

    def __init__(self, name: str):
        self.name = name
        self.log = open(WORK_DIR / f"{name}.log", "w")
        self.process = subprocess.Popen(
            [SHY, *member_args(name), "mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.log,
            text=True,
        )
        self.last_id = 0
        self.request("initialize", {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "shyspace-e2e", "version": "1"},
        })
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def send(self, message: dict) -> None:
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def request(self, method: str, params: dict) -> dict:
        self.last_id += 1
        self.send({"jsonrpc": "2.0", "id": self.last_id, "method": method, "params": params})
        while True:
            line = self.process.stdout.readline()
            if not line:
                raise RuntimeError(f"{self.name} exited; see {self.log.name}")
            reply = json.loads(line)
            if reply.get("id") != self.last_id:
                continue
            if "error" in reply:
                raise RuntimeError(f"{self.name} {method}: {reply['error']}")
            return reply["result"]

    def tool(self, name: str, **arguments) -> str:
        result = self.request("tools/call", {"name": name, "arguments": arguments})
        text = "\n".join(part.get("text", "") for part in result["content"])
        if result.get("isError"):
            raise RuntimeError(f"{self.name} {name}: {text}")
        return text

    def close(self) -> None:
        self.process.terminate()
        self.process.wait(timeout=10)
        self.log.close()


def member_args(name: str) -> list[str]:
    """A member's data directory and key file. Key files keep test runs out
    of the real system keychain."""
    return ["--dir", str(WORK_DIR / name), "--key-file", str(WORK_DIR / f"{name}.key")]


def check(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)
    print(f"ok  {message}")


def run_shy(*arguments: str, timeout: int = 120, stdin: str | None = None) -> str:
    environment = {**LOCAL_BUCKET, **os.environ}
    finished = subprocess.run(
        [SHY, *arguments], env=environment, input=stdin, capture_output=True, text=True, timeout=timeout
    )
    if finished.returncode != 0:
        raise RuntimeError(f"shy {' '.join(arguments[2:4])} failed:\n{finished.stderr}")
    return finished.stdout


def wait_for_request() -> str:
    """Polls the admin's invite list until the agent's request shows up."""
    deadline = time.time() + DELIVERY_TIMEOUT_SECONDS
    while time.time() < deadline:
        for line in run_shy(*member_args("admin"), "invite", "list").splitlines():
            if "Requested" in line and "from Claude" in line:
                return line.split()[0]
        time.sleep(1)
    raise AssertionError("the agent's join request never reached the admin")


def read_line_until(stream, wanted: str, what: str) -> list[str]:
    """Reads lines from `stream` until one contains `wanted`; returns them."""
    lines = []
    while True:
        line = stream.readline()
        if not line:
            raise AssertionError(f"{what} ended before saying {wanted!r}: {lines}")
        lines.append(line.rstrip("\n"))
        if wanted in line:
            return lines


def start_link(environment: dict, voucher_name: str = "admin",
               new_name: str = "laptop") -> tuple[subprocess.Popen, subprocess.Popen, list[list[str]], str]:
    """`voucher_name` shows a code and `new_name` asks through it; returns
    both processes, the voucher's numbered choices and the number the new
    device shows."""
    voucher = subprocess.Popen([SHY, *member_args(voucher_name), "link"], env=environment, stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    code = voucher.stdout.readline().strip()
    check(code.startswith("shyl1_") and "shysecret123" not in code,
          f"{voucher_name}'s device shows a link code that carries no bucket credentials")
    laptop = subprocess.Popen([SHY, *member_args(new_name), "link-join"], env=environment,
                              stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    laptop.stdin.write(code + "\n")
    laptop.stdin.close()
    choices = read_line_until(voucher.stderr, "None of these", "shy link")
    shown = read_line_until(laptop.stderr, "On your other device, pick", "shy link-join")[-1].rsplit("pick ", 1)[1]
    numbered = [parts for parts in (line.split() for line in choices) if len(parts) == 3 and parts[0] in ("1", "2", "3")]
    return voucher, laptop, numbered, shown


def a_wrong_pick_stops_the_link(environment: dict) -> None:
    voucher, laptop = None, None
    try:
        voucher, laptop, numbered, shown = start_link(environment)
        decoy = next(parts[0] for parts in numbered if " ".join(parts[1:]) != shown)
        voucher.stdin.write(decoy + "\n")
        voucher.stdin.close()
        check(voucher.wait(timeout=DELIVERY_TIMEOUT_SECONDS) != 0 and "wasn't the number" in voucher.stderr.read(),
              "picking another number ends the link on the admin's device")
        check(laptop.wait(timeout=DELIVERY_TIMEOUT_SECONDS) != 0 and "stopped the link" in laptop.stderr.read(),
              "the laptop is told at once, not after the answer link runs out")
        check(not (WORK_DIR / "laptop.linking").exists() and not (WORK_DIR / "laptop/shy.db").exists(),
              "the laptop keeps nothing from a stopped link")
    finally:
        for process in (voucher, laptop):
            if process and process.poll() is None:
                process.kill()


def a_linked_device_links_another(environment: dict) -> None:
    """The laptop, not the admin's device, links a tablet: it can't say the
    tablet was added until the admin's app is online and adds it."""
    voucher, tablet = None, None
    try:
        voucher, tablet, numbered, shown = start_link(environment, "laptop", "tablet")
        pick = next(parts[0] for parts in numbered if " ".join(parts[1:]) == shown)
        voucher.stdin.write(pick + "\n")
        voucher.stdin.close()
        check(voucher.wait(timeout=DELIVERY_TIMEOUT_SECONDS * 2) == 0
              and "next online" in voucher.stderr.read(),
              "the laptop doesn't claim the tablet was added while the admin's app is offline")
        run_shy(*member_args("admin"), "devices")
        check(tablet.wait(timeout=DELIVERY_TIMEOUT_SECONDS * 2) == 0,
              "the tablet joined once the admin's app was online and added it")
    finally:
        for process in (voucher, tablet):
            if process and process.poll() is None:
                process.kill()


def interrupting_the_pick_stops_the_link(environment: dict) -> None:
    voucher, laptop = None, None
    try:
        voucher, laptop, _numbered, _shown = start_link(environment)
        voucher.send_signal(signal.SIGINT)
        check(voucher.wait(timeout=DELIVERY_TIMEOUT_SECONDS) != 0,
              "Ctrl-C at the pick ends `shy link` at once, with standard input still open")
        check(laptop.wait(timeout=DELIVERY_TIMEOUT_SECONDS) != 0 and "stopped the link" in laptop.stderr.read(),
              "the laptop is told the link stopped")
        check(not (WORK_DIR / "laptop.linking").exists() and not (WORK_DIR / "laptop/shy.db").exists(),
              "the laptop keeps nothing from an interrupted link")
    finally:
        for process in (voucher, laptop):
            if process and process.poll() is None:
                process.kill()


def link_a_device() -> None:
    """The admin links a laptop of theirs by code, then removes it, each a
    real process on the real bucket: the person picks, on the device they
    use, the number the laptop shows, as they would by hand."""
    environment = {**LOCAL_BUCKET, **os.environ}
    a_wrong_pick_stops_the_link(environment)
    interrupting_the_pick_stops_the_link(environment)
    voucher, laptop = None, None
    try:
        voucher, laptop, numbered, shown = start_link(environment)
        pick = next(parts[0] for parts in numbered if " ".join(parts[1:]) == shown)
        check(len(numbered) == 3, "the admin's device shows three numbers to pick from")
        voucher.stdin.write(pick + "\n")
        voucher.stdin.close()
        check(voucher.wait(timeout=DELIVERY_TIMEOUT_SECONDS) == 0, "picking the laptop's number links it")
        check(laptop.wait(timeout=DELIVERY_TIMEOUT_SECONDS * 2) == 0, "the laptop joined once the admin's app added it")
    finally:
        for process in (voucher, laptop):
            if process and process.poll() is None:
                process.kill()
    devices = run_shy(*member_args("admin"), "devices")
    laptop_line = next(line for line in devices.splitlines() if line.startswith("Linux") or line.startswith("macOS"))
    laptop_id = laptop_line.split()[-1]
    check("This device, first device" in devices, "the admin lists the laptop among their devices")
    member = McpMember("laptop")
    try:
        check(member.tool("read_page", page="Getting started").startswith("# Welcome"),
              "the linked laptop reads the workspace's history")
        member.tool("send_message", channel="general", text="Hello from the laptop")
    finally:
        member.close()
    a_linked_device_links_another(environment)
    run_shy(*member_args("admin"), "devices", "remove", laptop_id)
    run_shy(*member_args("admin"), "devices")
    check(laptop_id not in run_shy(*member_args("admin"), "devices"), "the admin removed the laptop")


def main() -> int:
    if not Path(SHY).exists():
        print(f"shy binary not found at {SHY}; run `cargo build -p shy-cli`", file=sys.stderr)
        return 1
    shutil.rmtree(WORK_DIR, ignore_errors=True)
    WORK_DIR.mkdir(parents=True)

    run_shy(*member_args("admin"), "create", "--workspace", "E2E", "--name", "Ada")
    invite = run_shy(*member_args("admin"), "invite", "create", "--label", "e2e agent").strip()
    check(invite.startswith("shy3_") and "shysecret123" not in invite,
          "admin created a single-use invite that carries no bucket credentials")

    members: list[McpMember] = []
    joiner = None
    try:
        started = time.time()
        # The invite goes through standard input, the way that keeps it out of shell history.
        joiner = subprocess.Popen([SHY, *member_args("agent"), "join", "--name", "Claude", "--bot"],
                                  stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                                  stderr=open(WORK_DIR / "join.log", "w"), text=True)
        joiner.stdin.write(invite + "\n")
        joiner.stdin.close()
        token = wait_for_request()
        check(joiner.poll() is None, "the agent waits for approval")
        run_shy(*member_args("admin"), "invite", "approve", token)
        check(joiner.wait(timeout=DELIVERY_TIMEOUT_SECONDS) == 0,
              f"agent joined {time.time() - started:.1f}s after asking, once approved")
        check((WORK_DIR / "agent/workspace.json").exists(), "agent has the workspace, bucket settings included")

        admin = McpMember("admin")
        members.append(admin)
        agent = McpMember("agent")
        members.append(agent)
        tools = {tool["name"] for tool in agent.request("tools/list", {})["tools"]}
        check({"send_message", "read_page", "wait_for_messages"} <= tools, "agent exposes the MCP tools")
        check("#general" in agent.tool("list_channels"), "agent sees channels created before it joined")
        check(agent.tool("read_page", page="Getting started").startswith("# Welcome"), "agent reads history")

        time.sleep(GOSSIP_SETTLE_SECONDS)
        received: dict[str, str] = {}
        waiter = threading.Thread(target=lambda: received.update(
            text=admin.tool("wait_for_messages", channel="general", timeout_seconds=DELIVERY_TIMEOUT_SECONDS)))
        waiter.start()
        time.sleep(0.5)
        sent_at = time.time()
        agent.tool("send_message", channel="general", text="Hello from the agent")
        waiter.join()
        latency = time.time() - sent_at
        check("Claude (bot): Hello from the agent" in received.get("text", ""),
              f"admin received the agent's message after {latency:.2f}s")

        page_id = agent.tool("create_page", title="Plan", markdown="# Plan\n\n- [ ] ship it").split()[-1]
        # The page and its text are separate changes; wait until both arrived.
        deadline = time.time() + DELIVERY_TIMEOUT_SECONDS
        page_text = None
        while page_text != "# Plan\n\n- [ ] ship it" and time.time() < deadline:
            try:
                page_text = admin.tool("read_page", page=page_id)
            except RuntimeError:
                pass
            time.sleep(0.3)
        check(page_text == "# Plan\n\n- [ ] ship it", "admin reads the agent's new page")
        check(page_id in admin.tool("search", query="ship"), "admin finds the page with local search")

        roster = admin.tool("list_members")
        agent_line = next(line for line in roster.splitlines() if line.startswith("Claude"))
        check("[online]" in agent_line, "the admin sees the agent online, from its signed heartbeats")
        agent_id = agent_line.rsplit(": ", 1)[1]
        mentioned: dict[str, str] = {}
        waiter = threading.Thread(target=lambda: mentioned.update(
            text=agent.tool("wait_for_mentions", timeout_seconds=DELIVERY_TIMEOUT_SECONDS)))
        waiter.start()
        time.sleep(0.5)
        admin.tool("send_message", channel="general", text=f"[@Claude](shy://member/{agent_id}) can you review?")
        waiter.join()
        check("can you review?" in mentioned.get("text", ""), "the agent woke up when mentioned")
        check(agent.tool("wait_for_mentions", timeout_seconds=1) == "no new mentions",
              "each mention reaches the agent once")

        dm = admin.tool("open_dm", members=["Claude"]).split()[-1]
        admin.tool("send_message", channel=dm, text="PRIVATE-NOTE just for you")
        deadline = time.monotonic() + DELIVERY_TIMEOUT_SECONDS
        while dm not in agent.tool("list_channels") and time.monotonic() < deadline:
            time.sleep(0.5)
        check(f"DM with Ada ({dm})" in agent.tool("list_channels"), "the agent lists its DM with the admin")
        dm_text = agent.tool("read_messages", channel=dm)
        while "PRIVATE-NOTE" not in dm_text and time.monotonic() < deadline:
            time.sleep(0.5)
            dm_text = agent.tool("read_messages", channel=dm)
        check("PRIVATE-NOTE" in dm_text, "a DM reaches the agent, sealed for the two of them")
        replied: dict[str, str] = {}
        waiter = threading.Thread(target=lambda: replied.update(
            text=admin.tool("wait_for_messages", channel=dm, timeout_seconds=DELIVERY_TIMEOUT_SECONDS)))
        waiter.start()
        time.sleep(0.5)
        agent.tool("send_message", channel=dm, text="got it")
        waiter.join()
        check("got it" in replied.get("text", ""), "the admin gets the agent's reply in the DM")
    finally:
        if joiner and joiner.poll() is None:
            joiner.kill()
        for member in members:
            member.close()
    link_a_device()
    kit = run_shy(*member_args("admin"), "recovery-kit").strip()
    check(kit.startswith("shyrec1_"), "admin exported a recovery kit")
    restored = run_shy(*member_args("admin-restored"), "restore", stdin=kit)
    check("Restored the admin" in restored, "a new device restored the admin from the kit")
    invites = run_shy(*member_args("admin-restored"), "invite", "list")
    check("Accepted" in invites, "the restored admin still knows its invites")
    print("all end-to-end checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
