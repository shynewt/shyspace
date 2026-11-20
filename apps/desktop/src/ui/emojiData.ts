import type { EmojiData } from "frimousse";

/**
 * The newest Emoji version offered. frimousse checks what the system's font
 * can draw only for data it fetches itself; ours is bundled, so newer emoji
 * would show as blank boxes on older systems.
 */
const NEWEST_EMOJI_VERSION = 15.1;

const SKIN_TONES = ["light", "medium-light", "medium", "medium-dark", "dark"] as const;
type SkinToneName = (typeof SKIN_TONES)[number];

/** An emoji the app offers: frimousse's shape, plus the shortcodes typed after a colon. */
export type CatalogEmoji = EmojiData["emojis"][number] & { shortcodes: string[] };

export type EmojiCatalog = Omit<EmojiData, "emojis"> & { emojis: CatalogEmoji[] };

let catalog: Promise<EmojiCatalog> | null = null;

/**
 * Every emoji the app offers, loaded once and shared by the picker and the
 * colon suggestions. Emojibase's English data ships inside the app as lazily
 * loaded modules: the app may not connect anywhere but the core, so nothing is
 * fetched (see docs/decisions.md). This is the one place that builds the
 * categories, so a later category (like a workspace's own emoji) goes here.
 */
export function loadEmojiCatalog(): Promise<EmojiCatalog> {
  catalog ??= buildCatalog();
  return catalog;
}

/** The catalog in the shape frimousse takes. */
export async function loadEmojiData(): Promise<EmojiData> {
  const { emojis, ...rest } = await loadEmojiCatalog();
  return { ...rest, emojis: emojis.map(({ shortcodes: _shortcodes, ...emoji }) => emoji) };
}

async function buildCatalog(): Promise<EmojiCatalog> {
  const [{ default: emojis }, { default: messages }, { default: shortcodes }] = await Promise.all([
    import("emojibase-data/en/data.json"),
    import("emojibase-data/en/messages.json"),
    import("emojibase-data/en/shortcodes/github.json"),
  ]);
  const countryFlags = messages.subgroups.find((subgroup) => subgroup.key === "country-flag")?.order;
  const toneLabel = (key: SkinToneName) => {
    const tone = messages.skinTones.find((candidate) => candidate.key === key);
    if (!tone) throw new Error(`the emoji data has no ${key} skin tone`);
    return capitalize(tone.message);
  };
  const shortcodesOf = (hexcode: string): string[] => {
    const found = shortcodes[hexcode];
    return found === undefined ? [] : Array.isArray(found) ? found : [found];
  };
  return {
    locale: "en",
    categories: messages.groups
      .filter((group) => group.key !== "component")
      .map((group) => ({ index: group.order, label: capitalize(group.message) })),
    skinTones: {
      light: toneLabel("light"),
      "medium-light": toneLabel("medium-light"),
      medium: toneLabel("medium"),
      "medium-dark": toneLabel("medium-dark"),
      dark: toneLabel("dark"),
    },
    emojis: emojis.flatMap((emoji) => {
      if (emoji.group === undefined || emoji.version > NEWEST_EMOJI_VERSION) return [];
      const mixedTones = emoji.skins?.filter((skin) => Array.isArray(skin.tone)).map((skin) => skin.emoji) ?? [];
      return [
        {
          emoji: emoji.emoji,
          category: emoji.group,
          version: emoji.version,
          label: capitalize(emoji.label),
          tags: emoji.tags ?? [],
          countryFlag: (countryFlags !== undefined && emoji.subgroup === countryFlags) || undefined,
          skins: singleToneSkins(emoji.skins),
          aliases: mixedTones.length > 0 ? mixedTones : undefined,
          shortcodes: shortcodesOf(emoji.hexcode),
        },
      ];
    }),
  };
}

/**
 * Emoji matching `query`, best first: recently used ones, then by shortcode,
 * then by a word of the name, then by keyword (a whole one first).
 */
export function searchEmoji(
  emojis: CatalogEmoji[],
  query: string,
  recent: readonly string[],
  limit: number,
): CatalogEmoji[] {
  const wanted = query.toLowerCase();
  const scored = emojis.flatMap((emoji) => {
    const score = matchScore(emoji, wanted);
    if (score === 0) return [];
    const recency = recent.indexOf(emoji.emoji);
    return [{ emoji, score: recency === -1 ? score : 100 - recency }];
  });
  return scored
    .sort((left, right) => right.score - left.score)
    .slice(0, limit)
    .map(({ emoji }) => emoji);
}

function matchScore(emoji: CatalogEmoji, wanted: string): number {
  if (emoji.shortcodes.some((shortcode) => shortcode === wanted)) return 5;
  if (emoji.shortcodes.some((shortcode) => shortcode.startsWith(wanted))) return 4;
  const words = emoji.label.toLowerCase().split(/[\s:,-]+/);
  if (words.some((word) => word.startsWith(wanted))) return 3;
  if (emoji.tags.some((tag) => tag.toLowerCase() === wanted)) return 2.5;
  if (emoji.tags.some((tag) => tag.toLowerCase().startsWith(wanted))) return 2;
  if (emoji.label.toLowerCase().includes(wanted)) return 1;
  return 0;
}

/** The five single-tone variants by name, or none if the emoji doesn't have all five. */
function singleToneSkins(
  skins: Array<{ emoji: string; tone?: number | number[] }> | undefined,
): Record<SkinToneName, string> | undefined {
  const byTone = (index: number) => skins?.find((skin) => skin.tone === index + 1)?.emoji;
  const [light, mediumLight, medium, mediumDark, dark] = SKIN_TONES.map((_, index) => byTone(index));
  if (!light || !mediumLight || !medium || !mediumDark || !dark) return undefined;
  return { light, "medium-light": mediumLight, medium, "medium-dark": mediumDark, dark };
}

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}
