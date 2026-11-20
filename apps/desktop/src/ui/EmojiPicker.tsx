import {
  type EmojiPickerListCategoryHeaderProps,
  type EmojiPickerListEmojiProps,
  type EmojiPickerListRowProps,
  EmojiPicker as Picker,
} from "frimousse";
import { loadEmojiData } from "./emojiData";

/**
 * A searchable grid of emoji. Type to filter; the arrow keys and Enter pick
 * from the search box. Put it in a `Popover`.
 */
export function EmojiPicker({ onSelect }: { onSelect: (emoji: string) => void }) {
  return (
    <Picker.Root
      onEmojiSelect={({ emoji }) => onSelect(emoji)}
      resolveEmojiData={loadEmojiData}
      columns={8}
      className="isolate flex h-[320px] w-[290px] flex-col"
    >
      <Picker.Search
        aria-label="Search emoji"
        placeholder="Search emoji"
        className="mx-1 mt-1 h-8 shrink-0 rounded-lg border border-line bg-bg px-2.5 text-[13px] text-ink outline-none placeholder:text-faint focus:border-accent focus:ring-3 focus:ring-accent/15"
      />
      <Picker.Viewport className="relative mt-1 flex-1 outline-none">
        <Picker.Loading className="absolute inset-0 grid place-items-center text-[13px] text-muted">
          Loading emoji…
        </Picker.Loading>
        <Picker.Empty className="absolute inset-0 grid place-items-center text-[13px] text-muted">
          No emoji found.
        </Picker.Empty>
        <Picker.List className="pb-1 select-none" components={{ CategoryHeader, Row, Emoji }} />
      </Picker.Viewport>
    </Picker.Root>
  );
}

function CategoryHeader({ category, ...props }: EmojiPickerListCategoryHeaderProps) {
  return (
    <div className="bg-surface px-2 pt-2 pb-1 text-[11px] font-semibold tracking-wide text-muted uppercase" {...props}>
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
      className="grid size-[34px] place-items-center rounded-lg text-[20px] data-[active]:bg-surface-2"
      {...props}
    >
      {emoji.emoji}
    </button>
  );
}
