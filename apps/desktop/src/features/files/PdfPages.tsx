import { useEffect, useRef, useState } from "react";
import { MAX_PDF_PREVIEW_PAGES } from "../../lib/pdfPreview";

/** The most pixels one page may take, so a hostile page size can't exhaust memory. */
const MAX_IMAGE_PIXELS = 40_000_000;
/** Pages are drawn this wide (in CSS pixels), sharp on high-density screens. */
const PAGE_WIDTH_PX = 900;

type Rendered = { status: "rendering" } | { status: "shown"; pages: number } | { status: "failed" };

/**
 * A PDF's pages drawn as pictures with pdf.js, loaded only when a PDF is
 * opened. Nothing in the PDF runs: there's no scripting, no forms, and its
 * links aren't drawn as anything you can click.
 */
export function PdfPages({ bytes, name }: { bytes: Uint8Array; name: string }) {
  const container = useRef<HTMLDivElement>(null);
  const [rendered, setRendered] = useState<Rendered>({ status: "rendering" });

  useEffect(() => {
    let isCurrent = true;
    let destroy: (() => void) | null = null;
    (async () => {
      const [pdfjs, worker] = await Promise.all([
        import("pdfjs-dist"),
        import("pdfjs-dist/build/pdf.worker.min.mjs?url"),
      ]);
      pdfjs.GlobalWorkerOptions.workerSrc = worker.default;
      const task = pdfjs.getDocument({
        // A copy: the worker takes ownership of what it's given.
        data: bytes.slice(),
        enableXfa: false,
        useWasm: false,
        maxImageSize: MAX_IMAGE_PIXELS,
        stopAtErrors: false,
      });
      destroy = () => {
        task.destroy().catch(() => undefined);
      };
      const document = await task.promise;
      const count = Math.min(document.numPages, MAX_PDF_PREVIEW_PAGES);
      for (let number = 1; number <= count && isCurrent; number++) {
        const page = await document.getPage(number);
        const unscaled = page.getViewport({ scale: 1 });
        const scale = (PAGE_WIDTH_PX / unscaled.width) * Math.min(window.devicePixelRatio, 2);
        const viewport = page.getViewport({ scale });
        if (viewport.width * viewport.height > MAX_IMAGE_PIXELS) continue;
        const canvas = window.document.createElement("canvas");
        canvas.width = Math.floor(viewport.width);
        canvas.height = Math.floor(viewport.height);
        canvas.setAttribute("role", "img");
        canvas.setAttribute("aria-label", `Page ${number} of ${name}`);
        canvas.className = "mx-auto block w-full max-w-[900px] rounded-sm shadow-card";
        await page.render({ canvas, viewport }).promise;
        if (!isCurrent) return;
        container.current?.append(canvas);
      }
      if (isCurrent) setRendered({ status: "shown", pages: document.numPages });
    })().catch(() => isCurrent && setRendered({ status: "failed" }));
    return () => {
      isCurrent = false;
      destroy?.();
      container.current?.replaceChildren();
    };
  }, [bytes, name]);

  return (
    <div className="flex flex-col gap-3">
      <div ref={container} className="flex flex-col gap-3" />
      {rendered.status === "rendering" && <p className="text-sm text-muted">Drawing pages…</p>}
      {rendered.status === "failed" && (
        <p className="text-sm text-muted">This PDF couldn't be shown. Save it to open it with another app.</p>
      )}
      {rendered.status === "shown" && rendered.pages > MAX_PDF_PREVIEW_PAGES && (
        <p className="text-center text-sm text-muted">
          Showing the first {MAX_PDF_PREVIEW_PAGES} of {rendered.pages} pages. Save it to read the rest.
        </p>
      )}
    </div>
  );
}
