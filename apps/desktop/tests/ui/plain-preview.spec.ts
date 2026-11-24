/** One-line previews (Inbox, threads, quotes, search) name a link's real target, never the author's label. */
import { expect, test } from "@playwright/test";
import { plainPreview } from "../../src/lib/markdown";

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
