/**
 * Test-only entry: answers the app's `invoke` calls from the fake backend,
 * then starts the real app. By default you're Ada, the admin of "Acme";
 * `?start=onboarding` begins with no workspace, `?start=member` as Grace, a
 * regular member. `?seed=<text>` makes ids the same on every load.
 */
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { FakeBackend } from "./backend";
import { seedIds } from "./helpers";

declare global {
  interface Window {
    fake: FakeBackend;
    /** Set by a test (in an init script) to fill the fake before the app starts. */
    prepareFake?: (fake: FakeBackend) => void;
  }
}

mockIPC((command, args) => window.fake.handle(command, args), { shouldMockEvents: true });
mockWindows("main");
const params = new URLSearchParams(window.location.search);
const seed = params.get("seed");
if (seed) seedIds(seed);
const start = params.get("start");
window.fake =
  start === "onboarding"
    ? new FakeBackend()
    : start === "member"
      ? FakeBackend.asMember("Grace")
      : FakeBackend.withWorkspace("Acme", "Ada");

window.prepareFake?.(window.fake);

await import("../../../src/main");
