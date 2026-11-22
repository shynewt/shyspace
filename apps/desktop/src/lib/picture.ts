/** Pictures are shared at this size, square. */
const PICTURE_PX = 512;
const MAX_SHARED_BYTES = 256 * 1024;

/**
 * Crops an image file to its centre square and re-encodes it as a JPEG of
 * at most 512 px. Re-encoding drops everything but the pixels, like the
 * photo's location and camera.
 */
export async function squarePicture(file: Blob): Promise<Uint8Array> {
  const image = await createImageBitmap(file);
  const side = Math.min(image.width, image.height);
  const size = Math.min(side, PICTURE_PX);
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("couldn't prepare the picture");
  context.drawImage(image, (image.width - side) / 2, (image.height - side) / 2, side, side, 0, 0, size, size);
  image.close();
  for (const quality of [0.88, 0.75, 0.6, 0.45]) {
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/jpeg", quality));
    if (!blob) throw new Error("couldn't prepare the picture");
    if (blob.size <= MAX_SHARED_BYTES) return new Uint8Array(await blob.arrayBuffer());
  }
  throw new Error("that picture is too detailed to share; try another");
}
