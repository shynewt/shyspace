/** How picked names in the message box become links when sent. Runs without a browser. */
import { expect, test } from "@playwright/test";
import { withMentionLinks, withTypedMentions } from "../../src/lib/mentions";

const HERE = "0123456789abcdef";

test("picked people and pages become links, longest names first", () => {
  const people = new Map([
    ["Ada", "a1"],
    ["Ada Lovelace", "a2"],
  ]);
  const pages = new Map([
    ["Launch plan", "b1"],
    ["Ada", "b2"],
  ]);
  expect(withMentionLinks("@Ada Lovelace, @Ada and @Launch plan. @channel", HERE, people, pages)).toBe(
    `[@Ada Lovelace](shy://${HERE}/member/a2), [@Ada](shy://${HERE}/member/a1) and shy://${HERE}/page/b1. [@channel](shy://${HERE}/mention/channel)`,
  );
});

test("a page's title only links when it was picked, and as a whole word", () => {
  const pages = new Map([["Plan", "c1"]]);
  expect(withMentionLinks("@Planning and @Plan, not email@Plan", HERE, new Map(), pages)).toBe(
    `@Planning and shy://${HERE}/page/c1, not email@Plan`,
  );
});

test("a typed @name that is exactly one person's name is theirs, whatever the case", () => {
  const people = [
    { name: "Maya", id: "m1" },
    { name: "Mayank", id: "m2" },
  ];
  const typed = withTypedMentions("@maya, @MAYANK and @may, not email@maya", people);
  expect(typed.text).toBe("@Maya, @Mayank and @may, not email@maya");
  expect([...typed.named]).toEqual([
    ["Maya", "m1"],
    ["Mayank", "m2"],
  ]);
});

test("a name two people share stays plain text", () => {
  const people = [
    { name: "Sam", id: "s1" },
    { name: "sam", id: "s2" },
  ];
  expect(withTypedMentions("hi @sam", people)).toEqual({ text: "hi @sam", named: new Map() });
});
