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
  await app.$("*=The bucket works").waitForDisplayed(TIMEOUT);
  await app.$("button*=Create workspace").click();
  await app.$("aria/Message #general").waitForDisplayed(TIMEOUT);
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
      const inviteCode = await app.$('//*[starts-with(normalize-space(text()), "shy2_")]').getText();

      const agent = new CliMember(agentDir);
      const joined = agent.join(inviteCode.trim(), "Robot");
      await app.$("button=Codes match, let in").waitForClickable(TIMEOUT);
      await app.$("button=Codes match, let in").click();
      await joined;
      await app.keys("Escape");

      await agent.callTool("send_message", { channel: "general", text: "Hello from the agent" });
      await app.$("article*=Hello from the agent").waitForDisplayed(TIMEOUT);
      assert.ok(await app.$("article*=Robot").isDisplayed());
    });
  }));
