/**
 * Syntax colours for code in messages and pages. Both use CodeMirror's
 * language parsers (`@codemirror/language-data`, loaded per language on first
 * use) and these `tok-*` classes, styled in styles.css.
 */
import { LanguageDescription } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { highlightCode, tagHighlighter, tags } from "@lezer/highlight";

/** Longer code shows uncoloured: colouring costs time, and message code comes from others. */
const MAX_HIGHLIGHTED_CHARS = 20_000;

export const codeHighlighter = tagHighlighter([
  { tag: [tags.keyword, tags.modifier, tags.operatorKeyword, tags.controlKeyword], class: "tok-keyword" },
  { tag: [tags.string, tags.special(tags.string), tags.regexp, tags.character], class: "tok-string" },
  { tag: [tags.number, tags.bool, tags.atom, tags.null, tags.constant(tags.name)], class: "tok-number" },
  {
    tag: [
      tags.function(tags.variableName),
      tags.function(tags.propertyName),
      tags.function(tags.definition(tags.variableName)),
      tags.standard(tags.variableName),
      tags.macroName,
    ],
    class: "tok-function",
  },
  { tag: [tags.typeName, tags.className, tags.namespace, tags.tagName], class: "tok-type" },
  { tag: [tags.comment, tags.lineComment, tags.blockComment, tags.docComment], class: "tok-comment" },
  { tag: [tags.meta, tags.attributeName, tags.annotation], class: "tok-meta" },
]);

/** A run of code text, where it starts, and its colour class ("" for plain). */
export type CodeSegment = { text: string; from: number; className: string };

/** `code` split into coloured runs, or null when `language` isn't one we know. */
export async function highlight(code: string, language: string): Promise<CodeSegment[] | null> {
  if (!language || code.length > MAX_HIGHLIGHTED_CHARS) return null;
  const description = LanguageDescription.matchLanguageName(languages, language, true);
  if (!description) return null;
  const support = await description.load();
  const segments: CodeSegment[] = [];
  let from = 0;
  const put = (text: string, className: string) => {
    segments.push({ text, from, className });
    from += text.length;
  };
  highlightCode(code, support.language.parser.parse(code), codeHighlighter, put, () => put("\n", ""));
  return segments;
}
