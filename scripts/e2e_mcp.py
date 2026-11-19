#!/usr/bin/env python3
"""End-to-end test: two real `shy` processes, real networking, a real bucket.

An admin and a bot member talk over MCP. Checks joining, live delivery over
gossip, page creation and search across members.

Needs a running S3-compatible bucket (see docs/development.md) and a built CLI
(`cargo build -p shy-cli`). Settings come from the environment:
  SHY_S3_ENDPOINT, SHY_S3_BUCKET, SHY_S3_ACCESS_KEY_ID, SHY_S3_SECRET_ACCESS_KEY
  SHY_BIN   path to the shy binary (default: target/debug/shy)
  E2E_DIR   working directory (default: .local/e2e, wiped on each run)
"""

import json
import os
import shutil
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


def run_shy(*arguments: str, timeout: int = 120) -> str:
    environment = {**LOCAL_BUCKET, **os.environ}
    finished = subprocess.run([SHY, *arguments], env=environment, capture_output=True, text=True, timeout=timeout)
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


def main() -> int:
    if not Path(SHY).exists():
        print(f"shy binary not found at {SHY}; run `cargo build -p shy-cli`", file=sys.stderr)
        return 1
    shutil.rmtree(WORK_DIR, ignore_errors=True)
    WORK_DIR.mkdir(parents=True)

    run_shy(*member_args("admin"), "create", "--workspace", "E2E", "--name", "Ada")
    invite = run_shy(*member_args("admin"), "invite", "create", "--label", "e2e agent").strip()
    check(invite.startswith("shy2_") and "shysecret123" not in invite,
          "admin created a single-use invite that carries no bucket credentials")

    members: list[McpMember] = []
    joiner = None
    try:
        started = time.time()
        joiner = subprocess.Popen([SHY, *member_args("agent"), "join", "--name", "Claude", "--bot", invite],
                                  stdout=subprocess.DEVNULL, stderr=open(WORK_DIR / "join.log", "w"))
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
        deadline = time.time() + DELIVERY_TIMEOUT_SECONDS
        page_text = None
        while page_text is None and time.time() < deadline:
            try:
                page_text = admin.tool("read_page", page=page_id)
            except RuntimeError:
                time.sleep(0.3)
        check(page_text == "# Plan\n\n- [ ] ship it", "admin reads the agent's new page")
        check(page_id in admin.tool("search", query="ship"), "admin finds the page with local search")
    finally:
        if joiner and joiner.poll() is None:
            joiner.kill()
        for member in members:
            member.close()
    print("all end-to-end checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
