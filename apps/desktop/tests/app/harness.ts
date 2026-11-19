/**
 * Drives the real built app through tauri-driver (WebDriver), and the real
 * `shy` CLI as a second member. Needs a running S3 bucket; see
 * docs/development.md.
 */
import { type ChildProcess, spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { createInterface } from "node:readline";
import { remote } from "webdriverio";

declare global {
  namespace WebdriverIO {
    interface Capabilities {
      "tauri:options"?: { application: string };
    }
  }
}

const REPO = path.resolve(import.meta.dirname, "../../../..");
const APP_BINARY = process.env.SHY_APP ?? path.join(REPO, "target/debug/shyspace-desktop");
const CLI_BINARY = process.env.SHY_CLI ?? path.join(REPO, "target/debug/shy");
const DRIVER_PORT = 4444;
const NATIVE_DRIVER_PORT = 4445;

export const BUCKET = {
  endpoint: process.env.SHY_TEST_S3_ENDPOINT ?? "http://localhost:9000",
  bucket: process.env.SHY_TEST_S3_BUCKET ?? "shyspace",
  region: process.env.SHY_TEST_S3_REGION ?? "us-east-1",
  accessKey: process.env.SHY_TEST_S3_ACCESS_KEY ?? "shyadmin",
  secretKey: process.env.SHY_TEST_S3_SECRET_KEY ?? "shysecret123",
};

export type Browser = Awaited<ReturnType<typeof remote>>;

/** Runs `body` with fresh temporary folders, removed afterwards. */
export async function withTempDirs(purposes: string[], body: (dirs: string[]) => Promise<void>): Promise<void> {
  const dirs = purposes.map((purpose) => mkdtempSync(path.join(tmpdir(), `shyspace-${purpose}-`)));
  try {
    await body(dirs);
  } finally {
    for (const dir of dirs) rmSync(dir, { recursive: true, force: true });
  }
}

/** tauri-driver answers first; WebKit's own driver behind it a moment later. */
async function waitForDrivers(): Promise<void> {
  const deadline = Date.now() + 30_000;
  for (const port of [DRIVER_PORT, NATIVE_DRIVER_PORT]) {
    while (!(await isListening(port))) {
      if (Date.now() > deadline) throw new Error(`no WebDriver listening on port ${port}`);
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
  }
}

async function isListening(port: number): Promise<boolean> {
  try {
    return (await fetch(`http://127.0.0.1:${port}/status`)).ok;
  } catch {
    return false;
  }
}

/** Runs `body` against the app on `dataDir`, then quits the app. */
export async function withApp(dataDir: string, body: (app: Browser) => Promise<void>): Promise<void> {
  // tauri-driver hands its environment to the app, which reads SHY_DIR.
  const driver = spawn("tauri-driver", ["--port", String(DRIVER_PORT), "--native-port", String(NATIVE_DRIVER_PORT)], {
    env: { ...process.env, SHY_DIR: dataDir },
    stdio: ["ignore", "inherit", "inherit"],
  });
  try {
    await waitForDrivers();
    const browser = await remote({
      hostname: "127.0.0.1",
      port: DRIVER_PORT,
      logLevel: "warn",
      // WebKit's driver speaks classic WebDriver only, not BiDi.
      capabilities: { "wdio:enforceWebDriverClassic": true, "tauri:options": { application: APP_BINARY } },
    });
    try {
      await body(browser);
    } finally {
      await browser.deleteSession();
    }
  } finally {
    // Always, or a leftover driver holds the ports for the next test.
    await stop(driver);
  }
}

async function stop(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null) return;
  const exited = new Promise((resolve) => child.once("exit", resolve));
  child.kill();
  await exited;
}

/** A second member, played by the `shy` CLI with its own folder and key file. */
export class CliMember {
  private readonly dataDir: string;

  constructor(dataDir: string) {
    this.dataDir = dataDir;
  }

  private args(...command: string[]): string[] {
    return ["--dir", path.join(this.dataDir, "data"), "--key-file", path.join(this.dataDir, "key"), ...command];
  }

  /** Starts asking to join; resolves once the admin has approved. */
  join(invite: string, name: string): Promise<void> {
    const child = spawn(CLI_BINARY, this.args("join", "--bot", "--name", name, invite), { stdio: "inherit" });
    return new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", (code) => (code === 0 ? resolve() : reject(new Error(`shy join exited with ${code}`))));
    });
  }

  /** Calls one MCP tool through `shy mcp` and returns its text. */
  async callTool(tool: string, args: Record<string, unknown>): Promise<string> {
    const server = spawn(CLI_BINARY, this.args("mcp"), { stdio: ["pipe", "pipe", "inherit"] });
    const replies = createInterface({ input: server.stdout });
    const pending = new Map<number, (reply: { result?: { content: { text: string }[] }; error?: unknown }) => void>();
    replies.on("line", (line) => {
      const reply = JSON.parse(line);
      pending.get(reply.id)?.(reply);
    });
    let nextId = 0;
    const request = (method: string, params: unknown) => {
      nextId += 1;
      const id = nextId;
      server.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
      return new Promise<{ result?: { content: { text: string }[] }; error?: unknown }>((resolve) =>
        pending.set(id, resolve),
      );
    };
    try {
      await request("initialize", {
        protocolVersion: "2025-06-18",
        capabilities: {},
        clientInfo: { name: "app-tests", version: "1" },
      });
      server.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" })}\n`);
      const reply = await request("tools/call", { name: tool, arguments: args });
      if (!reply.result) throw new Error(`${tool} failed: ${JSON.stringify(reply.error)}`);
      return reply.result.content.map((part) => part.text).join("\n");
    } finally {
      server.stdin.end();
      await stop(server);
    }
  }
}
