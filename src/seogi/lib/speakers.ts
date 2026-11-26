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

/** A line of the transcript, as far as the helpers below need it. */
export interface SpokenLine {
  start_ms: number;
  end_ms: number;
  text: string;
  confidence?: number | null;
  unsure?: boolean | null;
  overlap?: boolean | null;
}

/** A line shorter than this (letters, not spaces or marks) says too little
 *  to tell a voice by. */
export const SAMPLE_MIN_CHARS = 6;

const letters = (text: string) =>
  text.replace(/[\s.,!?'"「」…·~-]/g, "").length;

/**
 * Up to `count` lines that show who a speaker is, from that speaker's
 * `lines`: long enough to hear (SAMPLE_MIN_CHARS), one from each part of
 * the meeting (beginning, middle, end of `durationMs`) where the speaker
 * talked, the most surely heard first. With too few long lines, shorter
 * ones fill in; a speaker with fewer lines gets as many as there are.
 * In the order said.
 */
export function sampleLines<T extends SpokenLine>(
  lines: T[],
  durationMs: number,
  count = 3,
): T[] {
  const span = Math.max(1, durationMs, ...lines.map((line) => line.end_ms));
  // Surely heard first, then the longer.
  const score = (line: T) =>
    (line.confidence ?? 1) -
    (line.unsure ? 0.5 : 0) -
    (line.overlap ? 0.5 : 0) +
    Math.min(letters(line.text), 60) / 1000;
  const ranked = [...lines].sort((a, b) => score(b) - score(a));
  const long = ranked.filter((line) => letters(line.text) >= SAMPLE_MIN_CHARS);
  const short = ranked.filter((line) => letters(line.text) < SAMPLE_MIN_CHARS);
  const part = (line: T) =>
    Math.min(count - 1, Math.floor((line.start_ms / span) * count));
  const chosen: T[] = [];
  // The best long line of each part of the meeting.
  for (let p = 0; p < count; p++) {
    const best = long.find((line) => part(line) === p);
    if (best) chosen.push(best);
  }
  // Then the best of what is left, long before short.
  for (const line of [...long, ...short]) {
    if (chosen.length >= count) break;
    if (!chosen.includes(line)) chosen.push(line);
  }
  return chosen.sort((a, b) => a.start_ms - b.start_ms);
}

/** One speaker's piece of the talking, as the share bar shows it. */
export interface SharePiece {
  speaker: number;
  /** Whole percent; the pieces add up to 100. */
  percent: number;
  talkMs: number;
}

/**
 * Whole percentages for the speakers' talking times that add up to 100
 * (the largest remainders round up), the one who talked most first.
 * Speakers who did not talk are left out.
 */
export function shareBar(
  talk: { speaker: number; talk_ms: number }[],
): SharePiece[] {
  const spoke = talk.filter((s) => s.talk_ms > 0);
  const total = spoke.reduce((n, s) => n + s.talk_ms, 0);
  if (total === 0) return [];
  const exact = spoke.map((s) => (s.talk_ms * 100) / total);
  const pieces = spoke.map((s, i) => ({
    speaker: s.speaker,
    percent: Math.floor(exact[i]),
    talkMs: s.talk_ms,
  }));
  let left = 100 - pieces.reduce((n, p) => n + p.percent, 0);
  const byRemainder = exact
    .map((value, i) => ({ i, rest: value - Math.floor(value) }))
    .sort((a, b) => b.rest - a.rest);
  for (const { i } of byRemainder) {
    if (left <= 0) break;
    pieces[i].percent += 1;
    left -= 1;
  }
  return pieces.sort((a, b) => b.talkMs - a.talkMs);
}

/** At most this share of the turns two speakers take may follow each other
 *  for them to look like one person heard as two. */
const SAME_PERSON_MEETS = 0.1;
/** Each must take at least this many turns for the pattern to mean much. */
const SAME_PERSON_TURNS = 3;

/**
 * A named speaker that speaker `id` may be the same person as, or null.
 * The speaker model sometimes hears one person as two (they turn away, the
 * room changes). Two people in a conversation answer each other; one person
 * heard as two seldom follows themself, while both halves talk with the
 * rest. So: a named speaker whose turns almost never meet `id`'s, both
 * talking with others. Only a suggestion for the person to confirm.
 */
export function maybeSamePerson(
  id: number,
  speakers: { id: number; name: string }[],
  order: number[],
  unnamed: (name: string) => boolean,
): number | null {
  // One turn per run of lines by the same speaker.
  const turns = order.filter(
    (speaker, i) => i === 0 || order[i - 1] !== speaker,
  );
  const count = (speaker: number) => turns.filter((s) => s === speaker).length;
  const meets = (a: number, b: number) =>
    turns.filter(
      (s, i) =>
        i > 0 &&
        ((s === a && turns[i - 1] === b) || (s === b && turns[i - 1] === a)),
    ).length;
  const withOthers = (a: number, b: number) =>
    turns.filter(
      (s, i) =>
        i > 0 &&
        (s === a || turns[i - 1] === a) &&
        s !== b &&
        turns[i - 1] !== b &&
        s !== turns[i - 1],
    ).length;
  if (count(id) < SAME_PERSON_TURNS) return null;
  let best: { id: number; ratio: number } | null = null;
  for (const other of speakers) {
    if (other.id === id || unnamed(other.name)) continue;
    if (count(other.id) < SAME_PERSON_TURNS) continue;
    if (withOthers(id, other.id) < 2 || withOthers(other.id, id) < 2) continue;
    const ratio = meets(id, other.id) / Math.min(count(id), count(other.id));
    if (ratio <= SAME_PERSON_MEETS && (!best || ratio < best.ratio)) {
      best = { id: other.id, ratio };
    }
  }
  return best?.id ?? null;
}
