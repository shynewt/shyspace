import type { ThemeSwatchColors } from "../ui";
import type { Palette } from "./preferences";

export const PALETTE_NAMES: Record<Palette, string> = {
  shy: "Shy",
  paper: "Paper",
  fjord: "Fjord",
  moss: "Moss",
  dusk: "Dusk",
  contrast: "Contrast",
};

/**
 * Each theme's own colours, for the cards that show every theme at once: the
 * page's tokens only hold the shown one. They must equal styles.css's.
 */
export const PALETTE_SWATCHES: Record<Palette, { light: ThemeSwatchColors; dark: ThemeSwatchColors }> = {
  shy: {
    light: { bg: "#f7f6f3", surface: "#ffffff", border: "#e4e1da", text: "#22201c", muted: "#68645d" },
    dark: { bg: "#141412", surface: "#1b1b19", border: "#2e2e2a", text: "#ecebe6", muted: "#9c9a92" },
  },
  paper: {
    light: { bg: "#f6f1e7", surface: "#fdf9f1", border: "#e2d8c4", text: "#2b241a", muted: "#6b5f4c" },
    dark: { bg: "#1c1813", surface: "#231e18", border: "#383026", text: "#ede4d3", muted: "#a89a84" },
  },
  fjord: {
    light: { bg: "#f2f5f8", surface: "#ffffff", border: "#d9e0e8", text: "#1d2733", muted: "#56657a" },
    dark: { bg: "#171c25", surface: "#1d232e", border: "#313a48", text: "#e6ebf2", muted: "#a3aebd" },
  },
  moss: {
    light: { bg: "#f2f4ef", surface: "#fcfdfa", border: "#d9ded1", text: "#1f261d", muted: "#5a6656" },
    dark: { bg: "#161b16", surface: "#1d231d", border: "#303830", text: "#e5ebe2", muted: "#9aa696" },
  },
  dusk: {
    light: { bg: "#f4f2f7", surface: "#fdfcfe", border: "#ddd8e6", text: "#251f2e", muted: "#635a70" },
    dark: { bg: "#1d1a24", surface: "#24202d", border: "#383242", text: "#ece7f2", muted: "#a79fb5" },
  },
  contrast: {
    light: { bg: "#ffffff", surface: "#ffffff", border: "#8a8a8a", text: "#000000", muted: "#3d3d3d" },
    dark: { bg: "#0d0d0d", surface: "#141414", border: "#7a7a7a", text: "#ffffff", muted: "#d0d0d0" },
  },
};
