import { useEffect, useRef, useState } from "react";
import { MAX_PDF_PREVIEW_PAGES } from "../../lib/pdfPreview";

/** The most pixels one page may take, so a hostile page size can't exhaust memory. */
const MAX_PAGE_PIXELS = 25_000_000;
/** The most pixels all drawn pages may take together (about 320 MB of canvas). */
const MAX_TOTAL_PIXELS = 80_000_000;
/** Pages are drawn this wide (in CSS pixels). */
const PAGE_WIDTH_PX = 900;
/** Sharper than 1.5 costs more memory than it shows. */
const MAX_PIXEL_RATIO = 1.5;
/** A PDF that takes longer than this to draw is given up on. */
const DRAWING_DEADLINE_MS = 30_000;

type Rendered =
  | { status: "rendering" }
  | { status: "shown"; pages: number; drawn: number }
  | { status: "failed"; reason: "broken" | "too-slow" };

/**
 * A PDF's pages drawn as pictures with pdf.js, loaded only when a PDF is
 * opened. Nothing in the PDF runs: there's no scripting, no forms, and its
 * links aren't drawn as anything you can click. Drawing stops at a pixel
 * budget and a deadline, so a hostile PDF can't hold the app.
 */
export function PdfPages({ bytes, name }: { bytes: Uint8Array; name: string }) {
  const container = useRef<HTMLDivElement>(null);
  const [rendered, setRendered] = useState<Rendered>({ status: "rendering" });

  useEffect(() => {
    const pages = container.current;
    if (!pages) return;
    let isCurrent = true;
    let destroy = () => {};
    const deadline = setTimeout(() => {
      if (!isCurrent) return;
      isCurrent = false;
      destroy();
      setRendered({ status: "failed", reason: "too-slow" });
    }, DRAWING_DEADLINE_MS);

    (async () => {
      const [pdfjs, worker] = await Promise.all([
        import("pdfjs-dist"),
        import("pdfjs-dist/build/pdf.worker.min.mjs?url"),
      ]);
      if (!isCurrent) return;
      pdfjs.GlobalWorkerOptions.workerSrc = worker.default;
      const task = pdfjs.getDocument({
        // A copy: the worker takes ownership of what it's given.
        data: bytes.slice(),
        enableXfa: false,
        useWasm: false,
        maxImageSize: MAX_PAGE_PIXELS,
        stopAtErrors: false,
      });
      destroy = () => {
        task.destroy().catch(() => undefined);
      };
      const document = await task.promise;
      if (!isCurrent) return destroy();
      const count = Math.min(document.numPages, MAX_PDF_PREVIEW_PAGES);
      let totalPixels = 0;
      let drawn = 0;
      for (let number = 1; number <= count; number++) {
        const page = await document.getPage(number);
        if (!isCurrent) return destroy();
        const unscaled = page.getViewport({ scale: 1 });
        const scale = (PAGE_WIDTH_PX / unscaled.width) * Math.min(window.devicePixelRatio, MAX_PIXEL_RATIO);
        const viewport = page.getViewport({ scale });
        const pixels = viewport.width * viewport.height;
        if (pixels > MAX_PAGE_PIXELS) {
          pages.append(note(`Page ${number} is too large to preview.`));
          drawn = number;
          continue;
        }
        if (totalPixels + pixels > MAX_TOTAL_PIXELS) break;
        totalPixels += pixels;
        const canvas = window.document.createElement("canvas");
        canvas.width = Math.floor(viewport.width);
        canvas.height = Math.floor(viewport.height);
        canvas.setAttribute("role", "img");
        canvas.setAttribute("aria-label", `Page ${number} of ${name}`);
        canvas.className = "mx-auto block w-full max-w-[900px] rounded-sm shadow-card";
        await page.render({ canvas, viewport }).promise;
        if (!isCurrent) return destroy();
        pages.append(canvas);
        drawn = number;
      }
      clearTimeout(deadline);
      setRendered({ status: "shown", pages: document.numPages, drawn });
    })().catch(() => {
      clearTimeout(deadline);
      if (isCurrent) setRendered({ status: "failed", reason: "broken" });
    });

    return () => {
      isCurrent = false;
      clearTimeout(deadline);
      destroy();
      for (const canvas of pages.querySelectorAll("canvas")) {
        // Frees the canvas's memory now rather than whenever it's collected.
        canvas.width = 0;
        canvas.height = 0;
      }
      pages.replaceChildren();
    };
  }, [bytes, name]);

  return (
    <div className="flex flex-col gap-3">
      <div ref={container} className="flex flex-col gap-3" />
      <p role="status" className="text-center text-sm text-muted">
        {rendered.status === "rendering" && "Drawing pages…"}
        {rendered.status === "failed" &&
          (rendered.reason === "too-slow"
            ? "This PDF takes too long to draw. Save it to open it with another app."
            : "This PDF couldn't be shown. Save it to open it with another app.")}
        {rendered.status === "shown" &&
          rendered.drawn < rendered.pages &&
          `Showing the first ${rendered.drawn} of ${rendered.pages} pages. Save it to read the rest.`}
      </p>
    </div>
  );
}

function note(text: string): HTMLParagraphElement {
  const paragraph = document.createElement("p");
  paragraph.className = "text-center text-sm text-muted";
  paragraph.textContent = text;
  return paragraph;
}
