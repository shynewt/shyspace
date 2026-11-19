/**
 * Test-only entry: answers the app's `invoke` calls from the fake backend,
 * then starts the real app. `?start=onboarding` begins with no workspace.
 */
import { mockIPC } from "@tauri-apps/api/mocks";
import { FakeBackend } from "./backend";

declare global {
  interface Window {
    fake: FakeBackend;
  }
}

mockIPC((command, args) => window.fake.handle(command, args), { shouldMockEvents: true });
const startsEmpty = new URLSearchParams(window.location.search).get("start") === "onboarding";
window.fake = startsEmpty ? new FakeBackend() : FakeBackend.withWorkspace("Acme", "Ada");

await import("../../../src/main");
