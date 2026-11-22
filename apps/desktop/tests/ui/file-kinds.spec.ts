/** The icon and label a file gets from its name and media type. Runs without a browser. */
import { expect, test } from "@playwright/test";
import { fileKindOf, fileLabelOf } from "../../src/lib/fileKinds";

test("common files get a kind from their extension, whatever their media type says", () => {
  expect(fileKindOf("report.pdf", "application/octet-stream")).toBe("document");
  expect(fileKindOf("notes.md", "text/markdown")).toBe("document");
  expect(fileKindOf("main.rs", "text/plain")).toBe("code");
  expect(fileKindOf("config.json", "application/json")).toBe("data");
  expect(fileKindOf("budget.xlsx", "")).toBe("spreadsheet");
  expect(fileKindOf("deck.pptx", "")).toBe("slides");
  expect(fileKindOf("backup.tar", "")).toBe("archive");
});

test("without a known extension the media type decides, then a plain file", () => {
  expect(fileKindOf("recording", "audio/ogg")).toBe("audio");
  expect(fileKindOf("README", "text/plain")).toBe("document");
  expect(fileKindOf("blob.xyz", "application/octet-stream")).toBe("other");
});

test("the label is a short extension in capitals, or nothing", () => {
  expect(fileLabelOf("report.pdf")).toBe("PDF");
  expect(fileLabelOf("notes.MD")).toBe("MD");
  expect(fileLabelOf("archive.tar.gz")).toBe("GZ");
  expect(fileLabelOf("README")).toBeNull();
  expect(fileLabelOf(".bashrc")).toBeNull();
  expect(fileLabelOf("paper.markdown")).toBeNull();
  expect(fileLabelOf("weird.ex e")).toBeNull();
});
