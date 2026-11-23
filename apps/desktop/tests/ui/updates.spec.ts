/** Updates: found and announced once, and looked for at launch. */
import { expect, test } from "@playwright/test";
import { type RestartOutcome, type UpdateCalls, UpdateFlow } from "../../src/lib/updateFlow";

/** Calls that answer `versions` in turn; a thrown value fails that check. */
function scripted(versions: (string | null | Error)[]) {
  const warnings: unknown[] = [];
  let checks = 0;
  const calls: UpdateCalls = {
    check: async () => {
      const next = versions[Math.min(checks, versions.length - 1)];
      checks += 1;
      if (next instanceof Error) throw next;
      return next ?? null;
    },
    restart: async (): Promise<RestartOutcome> => "restarting",
    warn: (error) => warnings.push(error),
  };
  return { calls, warnings, checks: () => checks };
}

test("a downloaded update is announced once and stays ready", async () => {
  const { calls } = scripted(["0.2.0"]);
  const flow = new UpdateFlow(calls);
  let announced = 0;
  flow.subscribe(() => {
    announced += 1;
  });

  await flow.checkNow();
  await flow.checkNow();

  expect(flow.readyVersion()).toBe("0.2.0");
  expect(announced).toBe(1);
});

test("with nothing newer, nothing is announced", async () => {
  const flow = new UpdateFlow(scripted([null]).calls);
  let announced = 0;
  flow.subscribe(() => {
    announced += 1;
  });
  await flow.checkNow();
  expect(flow.readyVersion()).toBeNull();
  expect(announced).toBe(0);
});

test("a failed check is only logged, and the next one tries again", async () => {
  const script = scripted([new Error("offline"), "0.2.0"]);
  const flow = new UpdateFlow(script.calls);

  await flow.checkNow();
  expect(flow.readyVersion()).toBeNull();
  expect(script.warnings).toHaveLength(1);

  await flow.checkNow();
  expect(flow.readyVersion()).toBe("0.2.0");
});

test("checks that overlap share one request", async () => {
  const script = scripted(["0.2.0"]);
  const flow = new UpdateFlow(script.calls);
  await Promise.all([flow.checkNow(), flow.checkNow(), flow.checkNow()]);
  expect(script.checks()).toBe(1);
});

test("the app looks for an update at launch", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.fake.updateChecks)).toBeGreaterThan(0);
});
