import assert from "node:assert/strict";
import { test } from "node:test";
import { type Browser, BUCKET, CliMember, withApp, withTempDirs } from "./harness.ts";

const TIMEOUT = { timeout: 30_000 };

async function createWorkspace(app: Browser) {
  await app.$("aria/Workspace").setValue("Acme");
  await app.$("aria/Your name").setValue("Ada");
  await app.$("aria/Endpoint").setValue(BUCKET.endpoint);
  await app.$("aria/Bucket").setValue(BUCKET.bucket);
  await app.$("aria/Region").setValue(BUCKET.region);
  await app.$("aria/Access key").setValue(BUCKET.accessKey);
  await app.$("aria/Secret key").setValue(BUCKET.secretKey);
  await app.$("button=Test connection").click();
  await app.$("span*=The bucket works").waitForDisplayed(TIMEOUT);
  await app.$("button*=Create workspace").click();
  await app.$("aria/Message #general").waitForDisplayed(TIMEOUT);
}

/** The full invite code on screen, however the page truncates it visually. */
async function waitForInviteCode(app: Browser): Promise<string> {
  let code: string | undefined;
  await app.waitUntil(
    async () => {
      code = await app.execute(() => document.body.textContent?.match(/shy2_[A-Za-z0-9_-]{40,}/)?.[0]);
      return Boolean(code);
    },
    { timeout: TIMEOUT.timeout, timeoutMsg: "no invite code appeared" },
  );
  if (!code) throw new Error("no invite code appeared");
  return code;
}

async function sendMessage(app: Browser, text: string) {
  await app.$("aria/Message #general").setValue(text);
  await app.keys("Enter");
  await app.$(`article*=${text}`).waitForDisplayed(TIMEOUT);
}

test("a new workspace keeps its messages after the app restarts", () =>
  withTempDirs(["restart"], async ([dataDir]) => {
    await withApp(dataDir, async (app) => {
      await createWorkspace(app);
      await sendMessage(app, "Written before the restart");
    });
    await withApp(dataDir, async (app) => {
      await app.$("aria/Message #general").waitForDisplayed(TIMEOUT);
      assert.ok(await app.$("article*=Written before the restart").isDisplayed());
    });
  }));

test("an approved agent's message arrives live in the app", () =>
  withTempDirs(["admin", "agent"], async ([adminDir, agentDir]) => {
    await withApp(adminDir, async (app) => {
      await createWorkspace(app);
      await app.$("button*=Members & invites").click();
      await app.$("aria/Who the invite is for").setValue("Robot");
      await app.$("button=Create").click();
      const inviteCode = await waitForInviteCode(app);

      const agent = new CliMember(agentDir);
      const joined = agent.join(inviteCode, "Robot");
      await app.$("button=Codes match, let in").waitForClickable(TIMEOUT);
      await app.$("button=Codes match, let in").click();
      await joined;
      await app.keys("Escape");

      await agent.callTool("send_message", { channel: "general", text: "Hello from the agent" });
      await app.$("article*=Hello from the agent").waitForDisplayed(TIMEOUT);
      assert.ok(await app.$("article*=Robot").isDisplayed());
    });
  }));
