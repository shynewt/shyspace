import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";

const SOURCE = fileURLToPath(new URL("../../src", import.meta.url));

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

function matchesIn(pattern: RegExp): string[] {
  return sourceFiles(SOURCE).flatMap((path) =>
    readFileSync(path, "utf8")
      .split("\n")
      .flatMap((line, index) =>
        [...line.matchAll(pattern)].map((match) => `${path.slice(SOURCE.length + 1)}:${index + 1} ${match[0]}`),
      ),
  );
}

test("font sizes come from the type scale", () => {
  expect(matchesIn(/text-\[[^\]]*\]/g)).toEqual([]);
});
