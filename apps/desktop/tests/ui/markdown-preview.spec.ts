/** Which parts of a page's markdown the live preview hides or replaces. Runs without a browser. */
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { ensureSyntaxTree } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { expect, test } from "@playwright/test";
import { type PreviewItem, previewItems } from "../../src/features/pages/editor/markdownPreview";

const HERE = "0123456789abcdef";

function itemsIn(text: string, ranges: { from: number; to: number }[], revealedLines: number[]): PreviewItem[] {
  const state = EditorState.create({ doc: text, extensions: markdown({ base: markdownLanguage }) });
  // Parsing on creation stops after a time budget; finish it so results don't depend on load.
  const tree = ensureSyntaxTree(state, text.length, 10_000);
  if (!tree) throw new Error("the markdown parser didn't finish");
  return previewItems(state.doc, tree, ranges, new Set(revealedLines), HERE);
}

function itemsOf(text: string, revealedLines: number[] = []): PreviewItem[] {
  return itemsIn(text, [{ from: 0, to: text.length }], revealedLines);
}

/** What a reader sees: hidden ranges removed, widgets drawn as symbols. */
function rendered(text: string, revealedLines: number[] = []): string {
  const replacements = itemsOf(text, revealedLines)
    .flatMap((item) => {
      const insert = shownAs(item);
      return insert === null || item.kind === "line" ? [] : [{ from: item.from, to: item.to, insert }];
    })
    .sort((left, right) => right.from - left.from);
  let output = text;
  for (const { from, to, insert } of replacements) output = output.slice(0, from) + insert + output.slice(to);
  return output;
}

function shownAs(item: PreviewItem): string | null {
  switch (item.kind) {
    case "hide":
      return "";
    case "bullet":
      return "•";
    case "checkbox":
      return item.isChecked ? "☑" : "☐";
    case "divider":
      return "———";
    case "page":
      return `<page ${item.pageId}>`;
    case "message":
      return `<message ${item.messageId}>`;
    case "image":
      return `<image ${item.fileId} ${item.width ?? "auto"} "${item.caption}">`;
    case "file":
      return `<file ${item.fileId}: ${item.label}>`;
    case "mention":
      return `<mention ${item.memberId}>`;
    case "elsewhere":
      return `<${item.link.kind} in ${item.link.workspace}>`;
    case "callout":
      return `<${item.callout}>`;
    case "line":
    case "style":
      return null;
  }
}

test("headings, emphasis, code and quotes lose their marks", () => {
  expect(rendered("# Title\n## Sub ##\nSome **bold**, *it*, _it_, ~~old~~ and `code`\n> quoted")).toBe(
    "Title\nSub\nSome bold, it, it, old and code\nquoted",
  );
});

test("marks show again on revealed lines only", () => {
  expect(rendered("# Title\nSome **bold**\n> quote", [2])).toBe("Title\nSome **bold**\nquote");
  expect(rendered("# Title\nSome **bold**\n> quote", [1, 3])).toBe("# Title\nSome bold\n> quote");
});

test("links keep their text and autolinks lose their brackets", () => {
  expect(rendered('[spec](https://example.com "Spec") and <https://example.com> and https://bare.example')).toBe(
    "spec and https://example.com and https://bare.example",
  );
});

test("bullets and checkboxes replace list markers; numbers stay", () => {
  expect(rendered("- one\n  - nested\n1. first\n- [ ] todo\n- [x] done")).toBe(
    "• one\n  • nested\n1. first\n☐ todo\n☑ done",
  );
  expect(rendered("- [ ] todo", [1])).toBe("- [ ] todo");
});

test("a checkbox is named by its item's text", () => {
  const [box] = itemsOf("- [x] Ship **the** app").filter((item) => item.kind === "checkbox");
  expect(box).toMatchObject({ kind: "checkbox", isChecked: true, label: "Ship the app" });
});

test("code is shown as written, between fences that hide until you edit them", () => {
  expect(rendered("`**not bold**`\n\n```\n# not a heading\n**raw**\n```")).toBe(
    "**not bold**\n\n\n# not a heading\n**raw**\n",
  );
  expect(rendered("```sh\ngit pull\n```")).toBe("sh\ngit pull\n");
  expect(rendered("```sh\ngit pull\n```", [1, 3])).toBe("```sh\ngit pull\n```");
  expect(itemsOf("```sh\ngit pull\n```")).toContainEqual({
    kind: "style",
    from: 3,
    to: 5,
    className: "cm-md-code-language",
  });
});

test("page links and mentions become chips, even on revealed lines", () => {
  const text = "[old](shy://page/ab12) shy://page/cd34 [@Ada](shy://member/ff00)";
  expect(rendered(text)).toBe("<page ab12> <page cd34> <mention ff00>");
  expect(rendered(text, [1])).toBe("<page ab12> <page cd34> <mention ff00>");
});

test("a mention chip knows who it names; @channel and @here are plain text in a page", () => {
  const items = itemsOf("[@Ada](shy://member/ff00) and [@channel](shy://mention/channel) [@here](shy://mention/here)");
  const mentions = items.filter((item) => item.kind === "mention");
  expect(mentions.map((item) => (item.kind === "mention" ? item.memberId : undefined))).toEqual(["ff00"]);
  expect(rendered("[@channel](shy://mention/channel) and [@here](shy://mention/here)")).toBe("@channel and @here");
});

test("a page link inside code stays text", () => {
  expect(rendered("`shy://page/ab12`")).toBe("shy://page/ab12");
});

test("a page file's image becomes an image, with its caption and width", () => {
  expect(rendered("![Team photo](shy://file/ab12#width=60)")).toBe('<image ab12 60 "Team photo">');
  expect(rendered("![](shy://file/ab12)")).toBe('<image ab12 auto "">');
  expect(rendered("![a \\[b\\] \\`c\\`](shy://file/ab12)")).toBe('<image ab12 auto "a [b] `c`">');
});

test("widths outside 10 to 100 and other fragments are ignored", () => {
  for (const fragment of ["#width=5", "#width=101", "#width=60.5", "#w=60", "#width=60&x=1"]) {
    expect(rendered(`![x](shy://file/ab12${fragment})`)).toBe('<image ab12 auto "x">');
  }
});

test("a link to a page file becomes a file chip; in code it stays text", () => {
  expect(rendered("see [q3 \\[draft\\].csv](shy://file/ab12) now")).toBe("see <file ab12: q3 [draft].csv> now");
  expect(rendered("`[x](shy://file/ab12)`")).toBe("[x](shy://file/ab12)");
});

test("images from elsewhere, and in code, stay text", () => {
  expect(rendered("![x](https://example.com/a.png)")).toBe("![x](https://example.com/a.png)");
  expect(rendered("`![x](shy://file/ab12)`")).toBe("![x](shy://file/ab12)");
});

test("message links become chips, whatever their label; inside code they stay text", () => {
  expect(rendered("see shy://message/ab12 or [the plan](shy://message/p:cd34/ef56)")).toBe(
    "see <message ab12> or <message p:cd34/ef56>",
  );
  expect(rendered("[](shy://message/ab12)")).toBe("<message ab12>");
  expect(rendered("`shy://message/ab12`")).toBe("shy://message/ab12");
});

test("a divider becomes a line and escapes lose their backslash", () => {
  expect(rendered("above\n\n---\n\\*literal\\*")).toBe("above\n\n———\n*literal*");
});

test("headings, quotes and code blocks style their whole lines", () => {
  const lineClasses = itemsOf("# One\n> two\n\n```\nthree\n```").flatMap((item) =>
    item.kind === "line" ? [item.className] : [],
  );
  expect(lineClasses).toEqual([
    "cm-md-h1",
    "cm-md-quote",
    "cm-md-codeblock cm-md-codeblock-first",
    "cm-md-codeblock",
    "cm-md-codeblock cm-md-codeblock-last",
  ]);
});

test("only the given ranges are looked at", () => {
  const text = "# One\n# Two";
  const items = itemsIn(text, [{ from: 0, to: 5 }], []);
  expect(items.filter((item) => item.kind === "hide")).toEqual([{ kind: "hide", from: 0, to: 2 }]);
});

test("nothing hidden spans a line break", () => {
  const text = "[a\nb](https://example.com) **x\ny**";
  for (const item of itemsOf(text)) {
    if (item.kind !== "line" && item.kind !== "style") expect(text.slice(item.from, item.to)).not.toContain("\n");
  }
});

test("an empty checked item or link label styles nothing, since empty styles break the editor", () => {
  for (const text of ["- [x] ", "- [x]\n", "[](https://example.com)"]) {
    for (const item of itemsOf(text)) {
      if (item.kind === "style") expect(item.to).toBeGreaterThan(item.from);
    }
  }
});

test("a callout's marker shows as its label, and its lines are coloured by kind", () => {
  const text = "> [!TIP]\n> Keep pull requests small.\n> Two approvals for payments.";
  expect(rendered(text)).toBe("<tip>\nKeep pull requests small.\nTwo approvals for payments.");
  const lines = itemsOf(text).flatMap((item) => (item.kind === "line" ? [item.className] : []));
  expect(lines).toEqual([
    "cm-md-callout callout-tip cm-md-callout-first",
    "cm-md-callout callout-tip",
    "cm-md-callout callout-tip cm-md-callout-last",
  ]);
});

test("a callout's marker shows as written on the line being edited, and unknown kinds stay quotes", () => {
  expect(rendered("> [!warning] Mind the keys", [1])).toBe("> [!warning] Mind the keys");
  expect(rendered("> [!INFO] Same as a note")).toBe("<note>Same as a note");
  expect(rendered("> [!FOO] just a quote")).toBe("[!FOO] just a quote");
  expect(
    itemsOf("> [!FOO] just a quote").some((item) => item.kind === "line" && item.className === "cm-md-quote"),
  ).toBe(true);
});

test("links naming this workspace are chips like older ones, and another's are marked as elsewhere", () => {
  const elsewhere = "fedcba9876543210";
  expect(rendered(`See shy://${HERE}/page/ab12 and [it](shy://${HERE}/message/cd34)`)).toBe(
    "See <page ab12> and <message cd34>",
  );
  expect(rendered(`See shy://${elsewhere}/page/ab12 and [it](shy://${elsewhere}/message/cd34)`)).toBe(
    `See <page in ${elsewhere}> and <message in ${elsewhere}>`,
  );
});

test("a mention of someone in another workspace is its plain text, never a mention chip", () => {
  const items = itemsOf("Thanks [@Ada](shy://fedcba9876543210/member/e1) again");
  expect(items.some((item) => item.kind === "mention")).toBe(false);
  expect(items.some((item) => item.kind === "style" && item.className === "cm-md-plain")).toBe(true);
  expect(rendered("Thanks [@Ada](shy://fedcba9876543210/member/e1) again")).toBe("Thanks @Ada again");
});
