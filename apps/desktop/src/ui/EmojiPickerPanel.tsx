import {
  type EmojiPickerListCategoryHeaderProps,
  type EmojiPickerListEmojiProps,
  type EmojiPickerListRowProps,
  EmojiPicker as Picker,
} from "frimousse";
import { useState } from "react";
import { type RecentEmoji, usePreferences, validRecentEmoji, withRecentEmoji } from "../lib/preferences";
import { loadEmojiData } from "./emojiData";

/**
 * The emoji picker itself, with frimousse and the emoji data. Loaded only
 * when a picker first opens (see `EmojiPicker`), so neither weighs on startup.
 */
export function EmojiPickerPanel({ onSelect }: { onSelect: (emoji: string) => void }) {
  const [preferences, update] = usePreferences();
  const [query, setQuery] = useState("");
  const recent = validRecentEmoji(preferences.recentEmoji);
  const choose = (picked: RecentEmoji) => {
    update({ recentEmoji: withRecentEmoji(recent, picked) });
    onSelect(picked.emoji);
  };
  return (
    <Picker.Root
      onEmojiSelect={({ emoji, label }) => choose({ emoji, label })}
      resolveEmojiData={loadEmojiData}
      columns={8}
      className="isolate flex h-emoji-picker w-emoji-picker flex-col"
    >
      <Picker.Search
        aria-label="Search emoji"
        placeholder="Search emoji"
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        className="mx-1 mt-1 h-8 shrink-0 rounded-lg border border-line bg-bg px-2.5 text-sm text-ink outline-none placeholder:text-faint focus:border-accent focus:ring-3 focus:ring-accent/15"
      />
      {recent.length > 0 && !query.trim() && <RecentRow recent={recent} onChoose={choose} />}
      <Picker.Viewport className="relative mt-1 flex-1 outline-none">
        <Picker.Loading className="absolute inset-0 grid place-items-center text-sm text-muted">
          Loading emoji…
        </Picker.Loading>
        <Picker.Empty className="absolute inset-0 grid place-items-center text-sm text-muted">
          No emoji found.
        </Picker.Empty>
        <Picker.List className="pb-1 select-none" components={{ CategoryHeader, Row, Emoji }} />
      </Picker.Viewport>
    </Picker.Root>
  );
}

function RecentRow({ recent, onChoose }: { recent: RecentEmoji[]; onChoose: (picked: RecentEmoji) => void }) {
  return (
    <div className="mt-1 shrink-0 border-b border-line px-1 pb-1">
      <div aria-hidden className="px-1 pt-1 pb-1 text-2xs font-semibold tracking-wide text-muted uppercase">
        Recently used
      </div>
      <ul aria-label="Recently used" className="grid grid-cols-8">
        {recent.map((entry) => (
          <li key={entry.emoji}>
            <button
              type="button"
              aria-label={entry.label}
              title={entry.label}
              onClick={() => onChoose(entry)}
              className="grid size-[34px] place-items-center rounded-lg text-xl hover:bg-surface-2"
            >
              {entry.emoji}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function CategoryHeader({ category, ...props }: EmojiPickerListCategoryHeaderProps) {
  return (
    <div className="bg-surface px-2 pt-2 pb-1 text-2xs font-semibold tracking-wide text-muted uppercase" {...props}>
      {category.label}
    </div>
  );
}

function Row({ children, ...props }: EmojiPickerListRowProps) {
  return (
    <div className="scroll-my-1 px-1" {...props}>
      {children}
    </div>
  );
}

function Emoji({ emoji, ...props }: EmojiPickerListEmojiProps) {
  return (
    <button
      type="button"
      className="grid size-[34px] place-items-center rounded-lg text-xl data-[active]:bg-surface-2"
      {...props}
    >
      {emoji.emoji}
    </button>
  );
}
