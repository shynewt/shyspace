import { avatarHue, initials } from "../ui/Avatar";

const SIZE = 128;

/**
 * A member's avatar as PNG bytes, drawn like the app's `Avatar`, for a desktop
 * notification's picture. Becomes their profile picture once members have one (#52).
 */
export async function avatarPng(id: string, name: string): Promise<number[]> {
  const canvas = document.createElement("canvas");
  canvas.width = SIZE;
  canvas.height = SIZE;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("can't draw the notification's picture");
  const hue = avatarHue(id);
  const gradient = context.createLinearGradient(0, 0, SIZE, SIZE);
  gradient.addColorStop(0, `hsl(${hue} 70% 62%)`);
  gradient.addColorStop(1, `hsl(${hue + 24} 62% 48%)`);
  context.fillStyle = gradient;
  context.beginPath();
  context.roundRect(0, 0, SIZE, SIZE, SIZE * 0.3);
  context.fill();
  context.fillStyle = "white";
  context.font = `600 ${SIZE * 0.38}px system-ui, sans-serif`;
  context.textAlign = "center";
  context.textBaseline = "middle";
  context.fillText(initials(name), SIZE / 2, SIZE / 2);
  const png = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!png) throw new Error("can't draw the notification's picture");
  return Array.from(new Uint8Array(await png.arrayBuffer()));
}
