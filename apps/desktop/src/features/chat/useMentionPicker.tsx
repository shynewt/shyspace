import { AtSign, Bot } from "lucide-react";
import { motion } from "motion/react";
import { useCallback, useMemo, useRef, useState } from "react";
import { api } from "../../lib/api";
import { mentionQueryAt, withMentionLinks } from "../../lib/mentions";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Avatar, cx, pop, springs } from "../../ui";

const MAX_SUGGESTIONS = 6;

type Suggestion = { name: string; id: string | null; isBot: boolean; detail: string };

const EVERYONE: Suggestion[] = [
  { name: "channel", id: null, isBot: false, detail: "Notify everyone in this channel" },
  { name: "here", id: null, isBot: false, detail: "Notify everyone online" },
];

/**
 * Suggests people as you type `@` in a textarea. Picked names show as plain
 * `@Name` while you write; `toSend` turns them into mention links.
 */
export function useMentionPicker(input: React.RefObject<HTMLTextAreaElement | null>, setText: (text: string) => void) {
  const { session } = useWorkspace();
  const loadMembers = useCallback(() => api.members(), []);
  const members = useLive(loadMembers, (change) => change.kind === "members") ?? [];
  const [query, setQuery] = useState<{ start: number; query: string } | null>(null);
  const [selected, setSelected] = useState(0);
  const picked = useRef(new Map<string, string>());

  const suggestions = useMemo(() => {
    if (!query) return [];
    const wanted = query.query.toLowerCase();
    const people: Suggestion[] = members
      .filter((member) => member.active && member.id !== session.me && member.name)
      .map((member) => ({
        name: member.name,
        id: member.id,
        isBot: member.is_bot,
        detail: member.is_bot ? "bot" : "",
      }));
    return [...people, ...EVERYONE]
      .filter((suggestion) => suggestion.name.toLowerCase().startsWith(wanted))
      .slice(0, MAX_SUGGESTIONS);
  }, [query, members, session.me]);

  const isOpen = suggestions.length > 0;

  const onTextChange = (text: string, caret: number) => {
    setQuery(mentionQueryAt(text, caret));
    setSelected(0);
  };

  const choose = (suggestion: Suggestion) => {
    const element = input.current;
    if (!element || !query) return;
    const caret = element.selectionStart;
    const before = `${element.value.slice(0, query.start)}@${suggestion.name} `;
    setText(before + element.value.slice(caret));
    if (suggestion.id) picked.current.set(suggestion.name, suggestion.id);
    setQuery(null);
    requestAnimationFrame(() => element.setSelectionRange(before.length, before.length));
  };

  /** Handles the keys the picker owns; returns whether it used the key. */
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
  const forgetPicked = () => picked.current.clear();

  const list = isOpen ? (
    <motion.div
      {...pop}
      transition={springs.snappy}
      id="mention-suggestions"
      role="listbox"
      aria-label="People to mention"
      className="absolute right-0 bottom-full left-0 mb-2 rounded-xl border border-line bg-surface p-1 shadow-float"
    >
      {suggestions.map((suggestion, index) => (
        <button
          key={suggestion.id ?? suggestion.name}
          type="button"
          role="option"
          id={`mention-option-${index}`}
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
          {suggestion.id ? (
            <Avatar id={suggestion.id} name={suggestion.name} isBot={suggestion.isBot} size={22} />
          ) : (
            <span className="grid h-[22px] w-[22px] place-items-center rounded-full bg-surface-2 text-muted">
              <AtSign size={13} />
            </span>
          )}
          <span className="font-medium">{suggestion.name}</span>
          {suggestion.isBot && <Bot size={13} className="text-accent" />}
          <span className="truncate text-[12px] text-faint">{suggestion.id ? "" : suggestion.detail}</span>
        </button>
      ))}
    </motion.div>
  ) : null;

  const inputProps = {
    "aria-autocomplete": "list" as const,
    "aria-expanded": isOpen,
    "aria-controls": isOpen ? "mention-suggestions" : undefined,
    "aria-activedescendant": isOpen ? `mention-option-${selected}` : undefined,
  };

  return { list, inputProps, onTextChange, onKeyDown, toSend, forgetPicked };
}
