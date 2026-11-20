/**
 * The `:name` being typed right before the caret, if it should suggest emoji:
 * at least two characters, after a space or the start, and not in code. So
 * `10:30`, `https://` and `:p` stay plain text.
 */
export function emojiQueryAt(text: string, caret: number): { start: number; query: string } | null {
  const before = text.slice(0, caret);
  const match = before.match(/(?:^|[\s(])(:([\p{L}\p{N}_+-]{2,}))$/u);
  if (!match || isInsideCode(before)) return null;
  return { start: caret - match[1].length, query: match[2] };
}

/** An odd number of backticks before the caret means it's inside a code span or block. */
function isInsideCode(before: string): boolean {
  return (before.match(/`/g)?.length ?? 0) % 2 === 1;
}
