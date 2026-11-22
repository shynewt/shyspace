/** Draws an image (any type the app shows) as PNG, the type clipboards take. */
async function asPng(url: string): Promise<Blob> {
  const image = new Image();
  image.src = url;
  await image.decode();
  const canvas = document.createElement("canvas");
  canvas.width = image.naturalWidth;
  canvas.height = image.naturalHeight;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("couldn't copy the image");
  context.drawImage(image, 0, 0);
  return new Promise((resolve, reject) =>
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("couldn't copy the image"))), "image/png"),
  );
}

/**
 * Puts an image on the system clipboard. The PNG is handed over as a
 * promise right away, because WebKit only allows writing during the click.
 */
export function copyImage(url: string): Promise<void> {
  return navigator.clipboard.write([new ClipboardItem({ "image/png": asPng(url) })]);
}
