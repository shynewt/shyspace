import type { MicState, MyHuddle, OutputState } from "../../lib/api";

/** "Ada", "Ada and Linus", "Ada, Linus and Grace", "Ada, Linus, Grace and 2 others". */
export function namesList(names: string[], limit = 3): string {
  if (names.length <= 1) return names.join("");
  if (names.length <= limit) return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
  const others = names.length - limit;
  return `${names.slice(0, limit).join(", ")} and ${others} ${others === 1 ? "other" : "others"}`;
}

const MIC_NEWS: Partial<Record<MicState, string>> = {
  permission_denied: "shyspace can't use your microphone",
  silent: "Your microphone sends no sound",
};

const OUTPUT_NEWS: Partial<Record<OutputState, string>> = {
  bluetooth_call_mode: "Your headset switched to call quality",
};

/**
 * What a screen reader should hear about your huddle going from `previous`
 * to `current`: joining and leaving, people coming and going, mute, and
 * device trouble. Never who's speaking: that changes too often to say. A
 * device unplugged is a toast instead (`unpluggedNews`), which is announced too.
 */
export function huddleNews(
  previous: MyHuddle | null,
  current: MyHuddle | null,
  nameOf: (member: string) => string,
  placeOf: (channel: string) => string,
): string[] {
  const wasIn = previous?.phase === "in";
  const isIn = current?.phase === "in";
  if (!current) return wasIn ? ["You left the huddle"] : [];
  if (!isIn) return [];
  if (!previous || !wasIn || previous.channel !== current.channel) {
    return [`You joined the huddle in ${placeOf(current.channel)}`];
  }
  const news: string[] = [];
  const before = new Set(previous.peers.map((peer) => peer.member));
  const after = new Set(current.peers.map((peer) => peer.member));
  for (const member of after) if (!before.has(member)) news.push(`${nameOf(member)} joined`);
  for (const member of before) if (!after.has(member)) news.push(`${nameOf(member)} left`);
  if (previous.muted !== current.muted) news.push(current.muted ? "Muted" : "Unmuted");
  const micNews = MIC_NEWS[current.mic];
  if (micNews && previous.mic !== current.mic) news.push(micNews);
  const outputNews = OUTPUT_NEWS[current.output];
  if (outputNews && previous.output !== current.output) news.push(outputNews);
  return news;
}

/** A toast for each device that went away between `previous` and `current`. */
export function unpluggedNews(previous: MyHuddle | null, current: MyHuddle | null): string[] {
  if (!previous || !current || previous.channel !== current.channel) return [];
  const news: string[] = [];
  if (current.mic === "lost" && previous.mic !== "lost") {
    news.push("Your microphone was disconnected. Switching to the default.");
  }
  if (current.output === "lost" && previous.output !== "lost") {
    news.push("Your speakers were disconnected. Switching to the default.");
  }
  return news;
}
