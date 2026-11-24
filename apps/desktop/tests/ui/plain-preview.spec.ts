/** One-line previews (Inbox, threads, quotes, search) name a link's real target, never the author's label. */
import { expect, test } from "@playwright/test";
import { cutPreview, plainPreview } from "../../src/lib/plainPreview";

const eve = "e".repeat(64);
const pageId = "a".repeat(32);
const titles = (id: string) => (id === pageId ? "Party planning" : undefined);
const names = (id: string) => (id === eve ? "Eve" : undefined);

test("a mention shows who it reaches, whatever its label says", () => {
  expect(plainPreview(`[@Ada](shy://member/${eve}) wire the money`, titles, names)).toBe("@Eve wire the money");
  expect(plainPreview(`ask [@Ada](shy://member/${"f".repeat(64)})`, titles, names)).toBe("ask @member ffffff");
  expect(plainPreview("[@here](shy://mention/channel) look", titles, names)).toBe("@channel look");
});

test("a labelled page or message link shows the page's title, or says it's a message", () => {
  expect(plainPreview(`read [Payroll](shy://page/${pageId})`, titles, names)).toBe("read Party planning");
  expect(plainPreview(`see [Approved](shy://message/${"b".repeat(64)})`, titles, names)).toBe("see a linked message");
});

test("a web link still shows its own text", () => {
  expect(plainPreview("see [the docs](https://example.com)", titles, names)).toBe("see the docs");
});

test("a label that already names its target keeps search's match marks", () => {
  const marked = `[@\u0001Eve\u0002](shy://member/${eve})`;
  expect(plainPreview(marked, titles, names)).toBe("@\u0001Eve\u0002");
  expect(plainPreview(`[@\u0001Ada\u0002](shy://member/${eve})`, titles, names)).toBe("@Eve");
});

test("however a link to someone is written, the preview names who it reaches", () => {
  const spellings = [
    `[@Ada](<shy://member/${eve}>) wire the money`,
    `[[@Ada]](shy://member/${eve}) wire the money`,
    `[@Ada\\]](shy://member/${eve}) wire the money`,
    `[@Ada](shy://member/${eve} "title") wire the money`,
    `[*@Ada*](shy://member/${eve}) wire the money`,
  ];
  for (const text of spellings) expect(plainPreview(text, titles, names), text).toBe("@Eve wire the money");
});

test("however a page link is written, the preview shows its title", () => {
  expect(plainPreview(`read [Payroll](<shy://page/${pageId}>)`, titles, names)).toBe("read Party planning");
  expect(plainPreview(`read [[Payroll]](shy://page/${pageId})`, titles, names)).toBe("read Party planning");
});

test("markdown marks drop out and blocks join with a dot", () => {
  expect(plainPreview("ship ~~Friday~~ Thursday", titles, names)).toBe("ship Friday Thursday");
  expect(plainPreview("# Plan\n- **ship** it\n- [x] test `v2`\n> quoted", titles, names)).toBe(
    "Plan · ship it · test v2 · quoted",
  );
  expect(plainPreview(`> [!note]\n> mind the gap, see shy://page/${pageId}`, titles, names)).toBe(
    "mind the gap, see Party planning",
  );
});

test("a reference-style link names what its definition points to", () => {
  expect(plainPreview(`[@Ada][x] wire\n\n[x]: shy://member/${eve}`, titles, names)).toBe("@Eve wire");
  expect(plainPreview(`[@Ada] wire\n\n[@Ada]: shy://member/${eve}`, titles, names)).toBe("@Eve wire");
  expect(plainPreview(`[@Ada][] now\n\n[@ada]: <shy://member/${eve}>`, titles, names)).toBe("@Eve now");
  expect(plainPreview(`read [Payroll][p]\n\n[p]: shy://page/${pageId}`, titles, names)).toBe("read Party planning");
});

test("a long message is read whole, so no link is ever cut", () => {
  const long = `${"word ".repeat(220)}[@Ada](shy://member/${eve})`;
  expect(plainPreview(long, titles, names)).not.toContain("Ada");
  const reference = `[@Ada][x] ${"word ".repeat(220)}\n\n[x]: shy://member/${eve}`;
  expect(plainPreview(reference, titles, names)).toMatch(/^@Eve word/);
});

test("a message too costly to format previews as the plain text the chat shows", () => {
  const costly = `[@Ada](shy://member/${eve}) ${"*a ".repeat(13000)}`;
  expect(plainPreview(costly, titles, names)).toMatch(/^\[@Ada\]\(shy:\/\/member\/e+\)/);
});

test("however a snippet ends inside a link, none of its label shows", () => {
  for (const snippet of [
    "ask [@Ada](shy://member/ee…",
    "ask [@Ada]…",
    "ask [[@Ada]](shy://me…",
    "ask [@Ada\nmore](shy://mem…",
    "ask [@Ada)](shy://m…",
    "ask [@Ada [z](https://a.com) word…",
    "ask [@Ada][x…",
    "ask it`s [@Ada…",
    "ask ``a ` b`` [@Ada…",
    "ask [@Ada](shy://member/eee (t)…",
  ]) {
    const preview = cutPreview(snippet, titles, names);
    expect(preview, snippet).not.toContain("Ada");
    expect(preview, snippet).toContain("ask");
  }
});

test("however a snippet starts inside a link, none of its label shows", () => {
  for (const snippet of [
    `…Ada](shy://member/${eve}) wire`,
    `…[x] Ada](shy://member/${eve}) wire`,
    `…da\nmore](shy://member/${eve}) wire`,
    "…Ada][x] wire",
    `…\`Ada](shy://member/${eve}) wire`,
  ]) {
    const preview = cutPreview(snippet, titles, names);
    expect(preview, snippet).not.toContain("da");
    expect(preview, snippet).toContain("wire");
  }
});

test("brackets that aren't a cut link stay", () => {
  expect(cutPreview("- [ ] buy milk and more…", titles, names)).toBe("buy milk and more…");
  expect(cutPreview("Done, see [1] maybe…", titles, names)).toBe("Done, see [1] maybe…");
  expect(cutPreview("`arr[0` is it…", titles, names)).toBe("arr[0 is it…");
  expect(cutPreview("[draft] still thinking", titles, names)).toBe("[draft] still thinking");
  expect(plainPreview(`> [!note]\n> ${"mind the gap ".repeat(100)}`, titles, names)).toMatch(/^mind the gap/);
});
