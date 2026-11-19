/**
 * Test-only entry: answers the app's `invoke` calls from the fake backend,
 * then starts the real app. By default you're Ada, the admin of "Acme";
 * `?start=onboarding` begins with no workspace, `?start=member` as Grace, a
 * regular member.
 */
import { mockIPC } from "@tauri-apps/api/mocks";
import { FakeBackend } from "./backend";

declare global {
  interface Window {
    fake: FakeBackend;
  }
}

mockIPC((command, args) => window.fake.handle(command, args), { shouldMockEvents: true });
const start = new URLSearchParams(window.location.search).get("start");
window.fake =
  start === "onboarding"
    ? new FakeBackend()
    : start === "member"
      ? FakeBackend.asMember("Grace")
      : FakeBackend.withWorkspace("Acme", "Ada");

await import("../../../src/main");
