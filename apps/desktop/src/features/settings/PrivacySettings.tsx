import { ChevronRight, Lock } from "lucide-react";
import { usePreferences } from "../../lib/preferences";
import { SegmentedControl } from "../../ui";
import { Setting } from "./layout";

export function PrivacySettings() {
  const [preferences, update] = usePreferences();
  return (
    <div className="flex flex-col gap-4">
      <div className="flex gap-3 rounded-xl border border-line bg-bg p-4">
        <div className="grid h-9 w-9 shrink-0 place-items-center rounded-xl bg-accent-soft text-accent-text">
          <Lock size={17} />
        </div>
        <div>
          <div className="text-base font-semibold">End-to-end encrypted</div>
          <p className="mt-0.5 text-sm leading-relaxed text-muted">
            Everything you write is encrypted on this device before it leaves. Only current members have the keys: the
            storage bucket and the network see scrambled data.
          </p>
        </div>
      </div>
      <details className="group rounded-xl border border-line px-4 py-3">
        <summary className="flex cursor-default list-none items-center gap-1.5 rounded-md text-sm font-medium [&::-webkit-details-marker]:hidden">
          <ChevronRight size={14} className="text-decorative transition-transform group-open:rotate-90" />
          How it works
        </summary>
        <ul className="mt-2 flex list-disc flex-col gap-1.5 pl-9 text-sm leading-relaxed text-muted">
          <li>When someone is removed, new keys are made that they never get. They keep what they already synced.</li>
          <li>Your copy on this device is encrypted too, with a key kept in your system's keychain.</li>
          <li>AI agents are members like anyone else: what they read goes to their AI provider.</li>
          <li>Desktop notifications show outside the encryption. You can hide their text under Notifications.</li>
        </ul>
      </details>
      <Setting
        title="Your cursor in pages"
        description="Whether others on the same page see that you're there and where you're typing. Appear offline also stops it. You still see theirs."
      >
        <SegmentedControl<"share" | "keep">
          label="Your cursor in pages"
          options={[
            { value: "share", label: "Share" },
            { value: "keep", label: "Don't share" },
          ]}
          value={preferences.shareCursorInPages ? "share" : "keep"}
          onChange={(choice) => update({ shareCursorInPages: choice === "share" })}
        />
      </Setting>
    </div>
  );
}
