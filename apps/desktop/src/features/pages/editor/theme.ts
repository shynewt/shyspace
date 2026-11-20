/** How pages look: the editor's frame, live-preview styles and syntax colors, all from design tokens. */
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags } from "@lezer/highlight";

const MONO = { fontFamily: "var(--font-mono)", fontSize: "0.88em" };

const highlightStyle = HighlightStyle.define([
  { tag: tags.strong, fontWeight: "650" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strikethrough, textDecoration: "line-through", color: "var(--text-muted)" },
  { tag: tags.link, color: "var(--accent-text)" },
  { tag: tags.url, color: "var(--text-muted)" },
  // Marks shown on the line being edited.
  { tag: [tags.processingInstruction, tags.meta, tags.contentSeparator, tags.labelName], color: "var(--text-faint)" },
  // Code inside fenced blocks.
  { tag: [tags.keyword, tags.modifier, tags.operatorKeyword], color: "var(--accent-text)" },
  { tag: [tags.string, tags.regexp], color: "var(--success-text)" },
  { tag: [tags.number, tags.bool, tags.atom], color: "var(--warning-text)" },
  { tag: [tags.comment, tags.lineComment, tags.blockComment], color: "var(--text-muted)", fontStyle: "italic" },
]);

const editorTheme = EditorView.theme({
  "&": { fontSize: "15.5px", backgroundColor: "transparent", color: "var(--text)" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font-sans)", lineHeight: "1.7", overflow: "visible" },
  ".cm-content": { padding: "0 0 40vh", caretColor: "var(--accent)" },
  ".cm-line": { padding: "0" },
  ".cm-cursor": { borderLeftColor: "var(--accent)", borderLeftWidth: "2px" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
    backgroundColor: "color-mix(in srgb, var(--accent) 22%, transparent) !important",
  },
  ".cm-placeholder": { color: "var(--text-muted)" },

  ".cm-md-h1": {
    fontSize: "1.75em",
    fontWeight: "700",
    letterSpacing: "-0.02em",
    lineHeight: "1.3",
    paddingTop: "0.7em",
  },
  ".cm-md-h2": {
    fontSize: "1.4em",
    fontWeight: "650",
    letterSpacing: "-0.01em",
    lineHeight: "1.35",
    paddingTop: "0.6em",
  },
  ".cm-md-h3": { fontSize: "1.18em", fontWeight: "620", lineHeight: "1.4", paddingTop: "0.5em" },
  ".cm-md-h4, .cm-md-h5, .cm-md-h6": { fontWeight: "600", paddingTop: "0.4em" },
  ".cm-md-quote": { borderLeft: "3px solid var(--surface-3)", paddingLeft: "0.85em", color: "var(--text-muted)" },
  ".cm-md-codeblock": { ...MONO, backgroundColor: "var(--surface-2)", paddingLeft: "0.9em", paddingRight: "0.9em" },
  ".cm-md-codeblock-first": { borderTopLeftRadius: "8px", borderTopRightRadius: "8px", paddingTop: "0.35em" },
  ".cm-md-codeblock-last": { borderBottomLeftRadius: "8px", borderBottomRightRadius: "8px", paddingBottom: "0.35em" },
  ".cm-md-code": {
    ...MONO,
    backgroundColor: "var(--surface-2)",
    border: "1px solid var(--border)",
    borderRadius: "5px",
    padding: "0.05em 0.3em",
  },
  ".cm-md-link": {
    color: "var(--accent-text)",
    textDecoration: "underline",
    textDecorationColor: "color-mix(in srgb, var(--accent) 40%, transparent)",
    textUnderlineOffset: "2px",
  },
  ".cm-md-indent": { fontFamily: "var(--font-mono)" },
  // Also over the syntax highlighter's span inside, which would draw list markers faint.
  ".cm-md-list-mark, .cm-md-list-number, .cm-md-list-mark *, .cm-md-list-number *": {
    color: "var(--text-muted)",
    fontVariantNumeric: "tabular-nums",
  },
  ".cm-md-bullet": {
    display: "inline-block",
    width: "1.1em",
    lineHeight: "1",
    textAlign: "center",
    "&::before": {
      content: '""',
      display: "inline-block",
      width: "0.34em",
      height: "0.34em",
      verticalAlign: "0.2em",
      borderRadius: "50%",
      backgroundColor: "var(--text-muted)",
    },
  },
  ".cm-md-bullet-1::before": { backgroundColor: "transparent", boxShadow: "inset 0 0 0 1.5px var(--text-muted)" },
  ".cm-md-bullet-2::before": { borderRadius: "1px" },
  ".cm-md-checkbox": {
    width: "1em",
    height: "1em",
    margin: "0 0.3em 0 0.05em",
    verticalAlign: "-0.15em",
    accentColor: "var(--accent)",
    cursor: "pointer",
  },
  ".cm-md-task-done": { color: "var(--text-muted)", textDecoration: "line-through" },
  ".cm-md-divider": {
    display: "inline-block",
    width: "100%",
    verticalAlign: "middle",
    borderTop: "1px solid var(--border)",
  },
  ".cm-md-page, .cm-md-mention": { borderRadius: "6px", padding: "0 0.3em", fontWeight: "500" },
  ".cm-md-page": {
    cursor: "pointer",
    color: "var(--accent-text)",
    backgroundColor: "var(--accent-soft)",
  },
  ".cm-md-page svg": { display: "inline", verticalAlign: "-0.12em", marginRight: "0.25em" },
  ".cm-md-page:hover": { backgroundColor: "color-mix(in srgb, var(--accent) 22%, transparent)" },
  ".cm-md-page-missing": { cursor: "default", color: "var(--text-muted)", backgroundColor: "var(--surface-2)" },
  ".cm-md-mention[role=button]": { cursor: "pointer" },
  ".cm-md-mention": {
    color: "var(--accent-text)",
    backgroundColor: "color-mix(in srgb, var(--accent) 10%, transparent)",
  },

  ".cm-tooltip": { border: "none", backgroundColor: "transparent" },
  ".cm-link-tooltip": {
    display: "flex",
    flexDirection: "column",
    maxWidth: "420px",
    padding: "4px 8px",
    borderRadius: "6px",
    backgroundColor: "var(--text)",
    color: "var(--bg)",
    boxShadow: "var(--shadow)",
    fontSize: "11.5px",
    lineHeight: "1.45",
  },
  ".cm-link-tooltip-url": { fontWeight: "500", overflowWrap: "anywhere" },
  ".cm-link-tooltip-hint": { opacity: "0.7" },
});

export const pageTheme = [editorTheme, syntaxHighlighting(highlightStyle)];
