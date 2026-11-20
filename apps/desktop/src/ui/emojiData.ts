import type { EmojiData } from "frimousse";

/**
 * The newest Emoji version offered. frimousse checks what the system's font
 * can draw only for data it fetches itself; ours is bundled, so newer emoji
 * would show as blank boxes on older systems.
 */
const NEWEST_EMOJI_VERSION = 15.1;

const SKIN_TONES = ["light", "medium-light", "medium", "medium-dark", "dark"] as const;
type SkinToneName = (typeof SKIN_TONES)[number];

/**
 * Emojibase's English data, shaped for frimousse. It ships inside the app as
 * a lazily loaded module: the app may not connect anywhere but the core, so
 * nothing is fetched (see docs/decisions.md).
 */
export async function loadEmojiData(): Promise<EmojiData> {
  const [{ default: emojis }, { default: messages }] = await Promise.all([
    import("emojibase-data/en/data.json"),
    import("emojibase-data/en/messages.json"),
  ]);
  const countryFlags = messages.subgroups.find((subgroup) => subgroup.key === "country-flag")?.order;
  const toneLabel = (key: SkinToneName) => {
    const tone = messages.skinTones.find((candidate) => candidate.key === key);
    if (!tone) throw new Error(`the emoji data has no ${key} skin tone`);
    return capitalize(tone.message);
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
        },
      ];
    }),
  };
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
