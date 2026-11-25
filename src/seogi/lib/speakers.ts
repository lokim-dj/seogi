// Which of the four quiet speaker tones (--sp1 … --sp4 in seogi.css) each
// speaker of a meeting wears: by the speaker's number, so a new name keeps
// the tone, and never the same as a speaker it often talks next to.

/** How many speaker tones the palette has. */
export const TONES = 4;

/**
 * A tone (1 … TONES) for every speaker id. The first four ids, in order,
 * get the four tones; a fifth or later takes the tone whose speakers it
 * sits next to least in `order` (the speakers of the lines, as said).
 */
export function speakerTones(
  ids: number[],
  order: number[],
): Map<number, number> {
  const sorted = [...new Set(ids)].sort((a, b) => a - b);
  const tones = new Map<number, number>();
  // How often two speakers' lines meet, either way round.
  const meets = new Map<string, number>();
  for (let i = 1; i < order.length; i++) {
    const [a, b] = [order[i - 1], order[i]];
    if (a === b) continue;
    const key = `${Math.min(a, b)}:${Math.max(a, b)}`;
    meets.set(key, (meets.get(key) ?? 0) + 1);
  }
  const met = (a: number, b: number) =>
    meets.get(`${Math.min(a, b)}:${Math.max(a, b)}`) ?? 0;
  sorted.forEach((id, rank) => {
    if (rank < TONES) {
      tones.set(id, rank + 1);
      return;
    }
    let best = (rank % TONES) + 1;
    let fewest = Number.MAX_SAFE_INTEGER;
    for (let tone = 1; tone <= TONES; tone++) {
      let count = 0;
      tones.forEach((other, otherId) => {
        if (other === tone) count += met(id, otherId);
      });
      if (count < fewest) {
        fewest = count;
        best = tone;
      }
    }
    tones.set(id, best);
  });
  return tones;
}
