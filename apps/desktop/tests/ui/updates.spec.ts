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

/** Calls whose restart answers only when `finish` is called. */
function heldRestart() {
  let restarts = 0;
  let finish: (outcome: RestartOutcome | Error) => void = () => {};
  const calls: UpdateCalls = {
    check: async () => "0.2.0",
    restart: () => {
      restarts += 1;
      return new Promise<RestartOutcome>((resolve, reject) => {
        finish = (outcome) => (outcome instanceof Error ? reject(outcome) : resolve(outcome));
      });
    },
    warn: () => {},
  };
  return { calls, restarts: () => restarts, finish: (outcome: RestartOutcome | Error) => finish(outcome) };
}

test("one restart runs at a time, and everyone sees it", async () => {
  const held = heldRestart();
  const flow = new UpdateFlow(held.calls);
  flow.restartToUpdate();
  flow.restartToUpdate();
  expect(held.restarts()).toBe(1);
  expect(flow.restartState()).toEqual({ kind: "restarting" });
});

test("a failed restart can be tried again, and keeps its reason until then", async () => {
  const held = heldRestart();
  const flow = new UpdateFlow(held.calls);
  flow.restartToUpdate();
  held.finish(new Error("the administrator password was declined"));
  await expect.poll(() => flow.restartState().kind).toBe("failed");
  flow.restartToUpdate();
  expect(held.restarts()).toBe(2);
  expect(flow.restartState()).toEqual({ kind: "restarting" });
});

test("a huddle holds the restart until after it, and a taken update says it's already updating", async () => {
  const held = heldRestart();
  const flow = new UpdateFlow(held.calls);
  flow.restartToUpdate();
  held.finish("in_huddle");
  await expect.poll(() => flow.restartState()).toEqual({ kind: "after_huddle" });
  flow.restartToUpdate();
  held.finish("nothing_ready");
  await expect.poll(() => flow.restartState()).toEqual({ kind: "already" });
});

test("the app looks for an update at launch", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.fake.updateChecks)).toBeGreaterThan(0);
});
