import { AtSign, Bot } from "lucide-react";
import { motion } from "motion/react";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";
import type { Member } from "../../lib/api";
import { emojiQueryAt } from "../../lib/emojiQuery";
import { useMembers } from "../../lib/members";
import { mentionQueryAt, withMentionLinks } from "../../lib/mentions";
import { usePreferences, validRecentEmoji, withRecentEmoji } from "../../lib/preferences";
import { reportError } from "../../lib/toast";
import { useWorkspace } from "../../lib/workspace";
import { Avatar, cx, pop, springs } from "../../ui";
import { type CatalogEmoji, type EmojiCatalog, loadEmojiCatalog, searchEmoji } from "../../ui/emojiData";
import { describeStatus, shownStatus } from "../members/StatusEmoji";

const MAX_SUGGESTIONS = 6;

/** Puts a mention of this member where the message box's cursor is. */
export type InsertMention = (name: string, memberId: string) => void;

type Person = { kind: "person"; name: string; member: Member | null; detail: string };
type EmojiSuggestion = { kind: "emoji"; emoji: CatalogEmoji };
type Suggestion = Person | EmojiSuggestion;

/** What's being typed before the caret: `@name` for people or `:name` for emoji. */
type Query = { kind: "person" | "emoji"; start: number; query: string };

const EVERYONE: Person[] = [
  { kind: "person", name: "channel", member: null, detail: "Notify everyone in this channel" },
  { kind: "person", name: "here", member: null, detail: "Notify everyone online" },
];

/**
 * Suggestions while typing in a textarea: people after `@`, emoji after `:`.
 * Picked names show as plain `@Name` while you write; `toSend` turns them into
 * mention links. Picked emoji go in as the emoji itself, from the app's set.
 */
export function useSuggestions(input: React.RefObject<HTMLTextAreaElement | null>, setText: (text: string) => void) {
  const { session } = useWorkspace();
  const members = useMembers() ?? [];
  const [preferences, updatePreferences] = usePreferences();
  const [query, setQuery] = useState<Query | null>(null);
  const [selected, setSelected] = useState(0);
  // Loaded on the first `:` query, not before: the emoji data is large.
  const [catalog, setCatalog] = useState<EmojiCatalog | null>(null);
  const picked = useRef(new Map<string, string>());
  // A channel and its open thread each have a composer.
  const listId = useId();

  const wantsEmoji = query?.kind === "emoji";
  useEffect(() => {
    if (wantsEmoji && !catalog) loadEmojiCatalog().then(setCatalog, reportError);
  }, [wantsEmoji, catalog]);

  const recentEmoji = validRecentEmoji(preferences.recentEmoji);
  const recentKey = recentEmoji.map((entry) => entry.emoji).join("\n");
  const suggestions = useMemo((): Suggestion[] => {
    if (!query) return [];
    if (query.kind === "emoji") {
      if (!catalog) return [];
      const recent = recentKey ? recentKey.split("\n") : [];
      return searchEmoji(catalog.emojis, query.query, recent, MAX_SUGGESTIONS).map((emoji) => ({
        kind: "emoji",
        emoji,
      }));
    }
    const wanted = query.query.toLowerCase();
    const people: Person[] = members
      .filter((member) => member.active && member.id !== session.me && member.name)
      .map((member) => ({ kind: "person", name: member.name, member, detail: "" }));
    return [...people, ...EVERYONE]
      .filter((suggestion) => suggestion.name.toLowerCase().startsWith(wanted))
      .slice(0, MAX_SUGGESTIONS);
  }, [query, members, session.me, catalog, recentKey]);

  const isOpen = suggestions.length > 0;

  const onTextChange = (text: string, caret: number) => {
    const mention = mentionQueryAt(text, caret);
    const emoji = mention ? null : emojiQueryAt(text, caret);
    setQuery(mention ? { kind: "person", ...mention } : emoji ? { kind: "emoji", ...emoji } : null);
    setSelected(0);
  };

  const choose = (suggestion: Suggestion) => {
    const element = input.current;
    if (!element || !query) return;
    const caret = element.selectionStart;
    let inserted: string;
    if (suggestion.kind === "emoji") {
      inserted = suggestion.emoji.emoji;
      updatePreferences({
        recentEmoji: withRecentEmoji(recentEmoji, { emoji: inserted, label: suggestion.emoji.label }),
      });
    } else {
      inserted = `@${suggestion.name} `;
      if (suggestion.member) picked.current.set(suggestion.name, suggestion.member.id);
    }
    const before = element.value.slice(0, query.start) + inserted;
    flushSync(() => {
      setText(before + element.value.slice(caret));
      setQuery(null);
    });
    // Now, not on the next frame: a key typed in between would land at the end, then the caret would jump back.
    element.setSelectionRange(before.length, before.length);
  };

  const insertMention: InsertMention = (name, memberId) => {
    const element = input.current;
    if (!element) return;
    const before = element.value.slice(0, element.selectionStart);
    const head = `${before}${before === "" || /\s$/.test(before) ? "" : " "}@${name} `;
    picked.current.set(name, memberId);
    setText(head + element.value.slice(element.selectionEnd));
    setQuery(null);
    element.focus();
    requestAnimationFrame(() => element.setSelectionRange(head.length, head.length));
  };

  /** Handles the keys the list owns; returns whether it used the key. */
  const onKeyDown = (event: React.KeyboardEvent): boolean => {
    if (!isOpen) return false;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      const step = event.key === "ArrowDown" ? 1 : -1;
      setSelected((index) => (index + step + suggestions.length) % suggestions.length);
    } else if (event.key === "Enter" || event.key === "Tab") {
      choose(suggestions[selected]);
    } else if (event.key === "Escape") {
      setQuery(null);
    } else {
      return false;
    }
    event.preventDefault();
    return true;
  };

  const toSend = (text: string) => withMentionLinks(text, picked.current);
  /** Shows a sent message's mention links as plain `@Name` again, remembering who they are. */
  const adopt = useCallback(
    (text: string) =>
      text.replace(
        /\[@([^\]]*)\]\(shy:\/\/(member|mention)\/([0-9a-z]+)\)/g,
        (_, name: string, kind: string, id: string) => {
          if (kind === "member") picked.current.set(name, id);
          return `@${name}`;
        },
      ),
    [],
  );
  const forgetPicked = () => picked.current.clear();

  const list = isOpen ? (
    <motion.div
      {...pop}
      transition={springs.snappy}
      id={listId}
      role="listbox"
      aria-label={query?.kind === "emoji" ? "Emoji" : "People to mention"}
      className="absolute right-0 bottom-full left-0 mb-2 rounded-xl border border-line bg-surface p-1 shadow-float"
    >
      {suggestions.map((suggestion, index) => (
        <button
          key={suggestion.kind === "emoji" ? suggestion.emoji.emoji : (suggestion.member?.id ?? suggestion.name)}
          type="button"
          role="option"
          id={`${listId}-${index}`}
          aria-selected={index === selected}
          onMouseDown={(event) => {
            event.preventDefault();
            choose(suggestion);
          }}
          onMouseMove={() => setSelected(index)}
          className={cx(
            "flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-[13px]",
            index === selected && "bg-surface-2",
          )}
        >
          {suggestion.kind === "emoji" ? <EmojiRow emoji={suggestion.emoji} /> : <PersonRow person={suggestion} />}
        </button>
      ))}
    </motion.div>
  ) : null;

  const inputProps = {
    "aria-autocomplete": "list" as const,
    "aria-expanded": isOpen,
    "aria-controls": isOpen ? listId : undefined,
    "aria-activedescendant": isOpen ? `${listId}-${selected}` : undefined,
  };

  return { list, inputProps, onTextChange, onKeyDown, toSend, adopt, forgetPicked, insertMention };
}

function PersonRow({ person }: { person: Person }) {
  const { member } = person;
  if (!member) {
    return (
      <>
        <span className="grid h-[22px] w-[22px] place-items-center rounded-full bg-surface-2 text-muted">
          <AtSign size={13} />
        </span>
        <span className="font-medium">{person.name}</span>
        <span className="truncate text-[12px] text-faint">{person.detail}</span>
      </>
    );
  }
  const status = shownStatus(member);
  return (
    <>
      <Avatar
        id={member.id}
        name={person.name}
        isBot={member.is_bot}
        size={22}
        presence={member.is_bot ? undefined : member.presence}
      />
      <span className="font-medium">{person.name}</span>
      {status?.emoji && (
        <span role="img" aria-label={`Status: ${describeStatus(status)}`}>
          {status.emoji}
        </span>
      )}
      {member.is_bot && <Bot size={13} className="text-accent" />}
    </>
  );
}

function EmojiRow({ emoji }: { emoji: CatalogEmoji }) {
  return (
    <>
      <span aria-hidden className="grid w-[22px] place-items-center text-[18px]">
        {emoji.emoji}
      </span>
      <span className="font-medium">{emoji.label}</span>
      {emoji.shortcodes[0] && <span className="truncate text-[12px] text-faint">:{emoji.shortcodes[0]}:</span>}
    </>
  );
}
