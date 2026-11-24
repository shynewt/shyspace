/** One-line previews (Inbox, threads, quotes, search) name a link's real target, never the author's label. */
import { expect, test } from "@playwright/test";
import { plainPreview } from "../../src/lib/plainPreview";

const HERE = "0123456789abcdef";
const eve = "e".repeat(64);
const pageId = "a".repeat(32);
const titles = (id: string) => (id === pageId ? "Party planning" : undefined);
const names = (id: string) => (id === eve ? "Eve" : undefined);

test("a mention shows who it reaches, whatever its label says", () => {
  expect(plainPreview(`[@Ada](shy://member/${eve}) wire the money`, titles, names, HERE)).toBe("@Eve wire the money");
  expect(plainPreview(`ask [@Ada](shy://member/${"f".repeat(64)})`, titles, names, HERE)).toBe("ask @member ffffff");
  expect(plainPreview("[@here](shy://mention/channel) look", titles, names, HERE)).toBe("@channel look");
});

test("a labelled page or message link shows the page's title, or says it's a message", () => {
  expect(plainPreview(`read [Payroll](shy://page/${pageId})`, titles, names, HERE)).toBe("read Party planning");
  expect(plainPreview(`see [Approved](shy://message/${"b".repeat(64)})`, titles, names, HERE)).toBe(
    "see a linked message",
  );
});

test("a web link still shows its own text", () => {
  expect(plainPreview("see [the docs](https://example.com)", titles, names, HERE)).toBe("see the docs");
});

test("however a link to someone is written, the preview names who it reaches", () => {
  const spellings = [
    `[@Ada](<shy://member/${eve}>) wire the money`,
    `[[@Ada]](shy://member/${eve}) wire the money`,
    `[@Ada\\]](shy://member/${eve}) wire the money`,
    `[@Ada](shy://member/${eve} "title") wire the money`,
    `[*@Ada*](shy://member/${eve}) wire the money`,
  ];
  for (const text of spellings) expect(plainPreview(text, titles, names, HERE), text).toBe("@Eve wire the money");
});

test("however a page link is written, the preview shows its title", () => {
  expect(plainPreview(`read [Payroll](<shy://page/${pageId}>)`, titles, names, HERE)).toBe("read Party planning");
  expect(plainPreview(`read [[Payroll]](shy://page/${pageId})`, titles, names, HERE)).toBe("read Party planning");
});

test("markdown marks drop out and blocks join with a dot", () => {
  expect(plainPreview("ship ~~Friday~~ Thursday", titles, names, HERE)).toBe("ship Friday Thursday");
  expect(plainPreview("# Plan\n- **ship** it\n- [x] test `v2`\n> quoted", titles, names, HERE)).toBe(
    "Plan · ship it · test v2 · quoted",
  );
  expect(plainPreview(`> [!note]\n> mind the gap, see shy://page/${pageId}`, titles, names, HERE)).toBe(
    "mind the gap, see Party planning",
  );
});

test("a reference-style link names what its definition points to", () => {
  expect(plainPreview(`[@Ada][x] wire\n\n[x]: shy://member/${eve}`, titles, names, HERE)).toBe("@Eve wire");
  expect(plainPreview(`[@Ada] wire\n\n[@Ada]: shy://member/${eve}`, titles, names, HERE)).toBe("@Eve wire");
  expect(plainPreview(`[@Ada][] now\n\n[@ada]: <shy://member/${eve}>`, titles, names, HERE)).toBe("@Eve now");
  expect(plainPreview(`read [Payroll][p]\n\n[p]: shy://page/${pageId}`, titles, names, HERE)).toBe(
    "read Party planning",
  );
});

test("a long message is read whole, so no link is ever cut", () => {
  const long = `${"word ".repeat(220)}[@Ada](shy://member/${eve})`;
  expect(plainPreview(long, titles, names, HERE)).not.toContain("Ada");
  const reference = `[@Ada][x] ${"word ".repeat(220)}\n\n[x]: shy://member/${eve}`;
  expect(plainPreview(reference, titles, names, HERE)).toMatch(/^@Eve word/);
});

test("a message too costly to format previews as the plain text the chat shows", () => {
  const costly = `[@Ada](shy://member/${eve}) ${"*a ".repeat(13000)}`;
  expect(plainPreview(costly, titles, names, HERE)).toMatch(/^\[@Ada\]\(shy:\/\/member\/e+\)/);
});
