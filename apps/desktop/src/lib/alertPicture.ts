import { avatarHue, initials } from "../ui/Avatar";
import { api } from "./api";
import { showableImageType } from "./imageHeaders";

const SIZE = 128;
const CORNER = SIZE * 0.3;
/** A notification shouldn't wait on a slow picture: past this, it shows the letter tile. */
const PICTURE_WAIT_MS = 1000;

/**
 * A member's avatar as PNG bytes, drawn like the app's `Avatar`, for a desktop
 * notification: their profile picture in the rounded square, or their letter
 * tile when they have none, or it can't load in time. `canLoadPicture` is false
 * for an alert from a workspace that isn't shown, whose pictures can't be asked for.
 */
export async function avatarPng(id: string, name: string, { canLoadPicture }: { canLoadPicture: boolean }) {
  const canvas = document.createElement("canvas");
  canvas.width = SIZE;
  canvas.height = SIZE;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("can't draw the notification's picture");
  context.beginPath();
  context.roundRect(0, 0, SIZE, SIZE, CORNER);
  context.clip();
  const picture = canLoadPicture ? await pictureOf(id) : null;
  if (picture) drawPicture(context, picture);
  else drawLetterTile(context, id, name);
  const png = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!png) throw new Error("can't draw the notification's picture");
  return Array.from(new Uint8Array(await png.arrayBuffer()));
}

/** Their picture, decoded; null when they have none, it isn't an image, or it's slow. */
async function pictureOf(id: string): Promise<ImageBitmap | null> {
  const loading = api.profilePicture(id).then((bytes) => {
    const type = showableImageType(bytes);
    return type ? createImageBitmap(new Blob([bytes as BlobPart], { type })) : null;
  });
  const timeout = new Promise<null>((resolve) => setTimeout(() => resolve(null), PICTURE_WAIT_MS));
  // A picture that fails to load is only a missing picture: the letter tile stands in.
  return Promise.race([loading.catch(() => null), timeout]);
}

/** Covers the square, cropped to its middle, as the app's avatars show it (`object-cover`). */
function drawPicture(context: CanvasRenderingContext2D, picture: ImageBitmap) {
  const scale = Math.max(SIZE / picture.width, SIZE / picture.height);
  const width = picture.width * scale;
  const height = picture.height * scale;
  context.drawImage(picture, (SIZE - width) / 2, (SIZE - height) / 2, width, height);
  picture.close();
}

function drawLetterTile(context: CanvasRenderingContext2D, id: string, name: string) {
  const hue = avatarHue(id);
  const gradient = context.createLinearGradient(0, 0, SIZE, SIZE);
  gradient.addColorStop(0, `hsl(${hue} 70% 62%)`);
  gradient.addColorStop(1, `hsl(${hue + 24} 62% 48%)`);
  context.fillStyle = gradient;
  context.fillRect(0, 0, SIZE, SIZE);
  context.fillStyle = "white";
  context.font = `600 ${SIZE * 0.38}px system-ui, sans-serif`;
  context.textAlign = "center";
  context.textBaseline = "middle";
  context.fillText(initials(name), SIZE / 2, SIZE / 2);
}
