/**
 * Work that must reach the shown workspace before another is shown: a draft,
 * page typing, a read position. Commands act on whichever workspace is
 * selected, so saving these once the switch has happened would send them to
 * the wrong one, or nowhere.
 */

const savers = new Set<() => Promise<void>>();

/** Keeps `save` to run before a switch, until the returned function is called. */
export function saveBeforeSwitch(save: () => Promise<void>): () => void {
  savers.add(save);
  return () => savers.delete(save);
}

/** Runs every pending save and waits for them; a failed one doesn't hold the switch back (it reports itself). */
export async function saveEverythingBeforeSwitch(): Promise<void> {
  await Promise.allSettled([...savers].map((save) => save()));
}
