/** Reading an image's claimed size before it is decoded. Runs without a browser. */
import { expect, test } from "@playwright/test";
import { showableImageType } from "../../src/lib/imageHeaders";

const be16 = (value: number) => [value >> 8, value & 0xff];
const le16 = (value: number) => [value & 0xff, value >> 8];

/** A minimal JPEG: start, an APP0 segment, then a frame header of the given size. */
function jpeg(width: number, height: number, beforeFrame: number[] = []): Uint8Array {
  const app0 = [0xff, 0xe0, ...be16(16), ...Array(14).fill(0)];
  const frame = [0xff, 0xc0, ...be16(17), 8, ...be16(height), ...be16(width), 3, ...Array(9).fill(0)];
  return new Uint8Array([0xff, 0xd8, ...app0, ...beforeFrame, ...frame]);
}

function gif(screen: [number, number], frame: [number, number]): Uint8Array {
  const header = [0x47, 0x49, 0x46, 0x38, 0x39, 0x61, ...le16(screen[0]), ...le16(screen[1]), 0, 0, 0];
  const comment = [0x21, 0xfe, 3, 0x61, 0x62, 0x63, 0];
  const descriptor = [0x2c, 0, 0, 0, 0, ...le16(frame[0]), ...le16(frame[1]), 0];
  return new Uint8Array([...header, ...comment, ...descriptor]);
}

test("an ordinary JPEG or GIF is shown", () => {
  expect(showableImageType(jpeg(1200, 800))).toBe("image/jpeg");
  expect(showableImageType(gif([400, 300], [400, 300]))).toBe("image/gif");
});

test("a JPEG claiming a huge frame is refused", () => {
  expect(showableImageType(jpeg(23_000, 23_000))).toBeNull();
});

test("fill bytes before a marker can't hide a JPEG's real frame", () => {
  // Decoders skip 0xFF fill bytes; reading them as a marker would land on a
  // small decoy frame further on and miss the real, huge one.
  // A parser that took "FF C0 00" as a length would jump 49,154 bytes, right
  // onto the decoy.
  const real = [0xff, 0xff, 0xc0, ...be16(17), 8, ...be16(23_000), ...be16(23_000), 3, ...Array(9).fill(0)];
  const padding = Array(49_154 - real.length).fill(0);
  expect(showableImageType(jpeg(16, 16, [...real, ...padding]))).toBeNull();
  // Standalone markers (no length) are stepped over, not read as lengths.
  expect(showableImageType(jpeg(23_000, 23_000, [0xff, 0xd0, 0xff, 0x01]))).toBeNull();
  expect(showableImageType(jpeg(640, 480, [0xff, 0xd0]))).toBe("image/jpeg");
});

test("a GIF whose first frame is larger than its screen is judged by the frame", () => {
  expect(showableImageType(gif([1, 1], [23_000, 23_000]))).toBeNull();
});

test("short, broken or unknown bytes are never shown and never throw", () => {
  for (const bytes of [
    [],
    [0xff, 0xd8],
    [0xff, 0xd8, 0xff],
    [0xff, 0xd8, 0xff, 0xff, 0xff],
    [0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0x80, 0, 0],
    [0x3c, 0x73, 0x76, 0x67, 0x3e],
  ]) {
    expect(showableImageType(new Uint8Array(bytes))).toBeNull();
  }
});
