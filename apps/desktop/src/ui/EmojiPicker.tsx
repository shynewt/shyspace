import { lazy, Suspense } from "react";

const EmojiPickerPanel = lazy(() =>
  import("./EmojiPickerPanel").then((module) => ({ default: module.EmojiPickerPanel })),
);

/**
 * A searchable grid of emoji, recently used ones first. Type to filter; the
 * arrow keys and Enter pick from the search box. Put it in a `Popover`.
 */
export function EmojiPicker({ onSelect }: { onSelect: (emoji: string) => void }) {
  return (
    <Suspense
      fallback={
        <div className="grid h-[320px] w-[290px] place-items-center text-[13px] text-muted">Loading emoji…</div>
      }
    >
      <EmojiPickerPanel onSelect={onSelect} />
    </Suspense>
  );
}
