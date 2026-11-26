// Naming who spoke from a few of their lines, one speaker at a time: a line
// is heard and a name chosen ("모르겠어요" counts for nothing), up to five;
// when four say the same name the whole speaker is that person, and the
// lines given another name go to that one alone.

import { sampleLines, type SpokenLine } from "./speakers";

/** How many of a speaker's lines are asked about, at most. */
export const ASK_LINES = 5;
/** How many answers with the same name settle who the speaker is. */
export const SAME_ENOUGH = 4;
/** Lines held back for the ones answered "모르겠어요". */
const SPARE_LINES = 3;

/**
 * The lines to ask about for one speaker: ASK_LINES clear ones from across
 * the meeting in the order said, then a few spare ones, best first, in case
 * some cannot be told.
 */
export function askLines<T extends SpokenLine>(
  lines: T[],
  durationMs: number,
): T[] {
  const first = sampleLines(lines, durationMs, ASK_LINES);
  const rest = lines.filter((line) => !first.includes(line));
  return [...first, ...sampleLines(rest, durationMs, SPARE_LINES)];
}

/** One line heard and who the person said it was (null: "모르겠어요"). */
export interface Answer<T> {
  line: T;
  name: string | null;
}

/** Where the asking about one speaker stands. */
export interface Asking<T> {
  lines: T[];
  answers: Answer<T>[];
  /** "하나 더 들어 볼게요" was pressed and no line answered since. */
  more: boolean;
}

export function startAsking<T>(lines: T[]): Asking<T> {
  return { lines, answers: [], more: false };
}

/** The line now asked about gets `name` (null: "모르겠어요"). */
export function answer<T>(state: Asking<T>, name: string | null): Asking<T> {
  const line = state.lines[state.answers.length];
  if (line === undefined) return state;
  const tidy = name?.trim() || null;
  return {
    ...state,
    answers: [...state.answers, { line, name: tidy }],
    more: false,
  };
}

/** One more line, though the answers so far would do. */
export function askMore<T>(state: Asking<T>): Asking<T> {
  return linesLeft(state) > 0 ? { ...state, more: true } : state;
}

export function linesLeft<T>(state: Asking<T>): number {
  return state.lines.length - state.answers.length;
}

/** How many answers gave a name (the count on the progress line). */
export function named<T>(state: Asking<T>): number {
  return state.answers.filter((a) => a.name).length;
}

/** What the answers so far say. */
export type Verdict<T> =
  /** Not enough yet: the next line is asked. */
  | { kind: "asking" }
  /** The speaker is `name` (`agree` of `of` answers); the lines in `move`
   *  were given someone else and go to that one. */
  | {
      kind: "decide";
      name: string;
      agree: number;
      of: number;
      move: Answer<T>[];
    }
  /** The answers went different ways: two or more people under one number.
   *  Only the lines answered move. */
  | { kind: "mixed"; move: Answer<T>[] }
  /** No name came of it (every line "모르겠어요", or none to hear). */
  | { kind: "none" };

export function verdict<T>(state: Asking<T>): Verdict<T> {
  const left = linesLeft(state);
  if (state.more && left > 0) return { kind: "asking" };
  const given = state.answers.filter(
    (a): a is Answer<T> & { name: string } => a.name !== null,
  );
  const counts = new Map<string, number>();
  for (const a of given) counts.set(a.name, (counts.get(a.name) ?? 0) + 1);
  // The name given most; on a tie, the one given first.
  let best: string | null = null;
  for (const a of given) {
    if (best === null || (counts.get(a.name) ?? 0) > (counts.get(best) ?? 0))
      best = a.name;
  }
  const agree = best ? (counts.get(best) ?? 0) : 0;
  const decide = (name: string): Verdict<T> => ({
    kind: "decide",
    name,
    agree,
    of: given.length,
    move: given.filter((a) => a.name !== name),
  });
  if (best && agree >= SAME_ENOUGH) return decide(best);
  if (given.length < ASK_LINES && left > 0) return { kind: "asking" };
  // Five answered, or nothing more to hear.
  if (!best) return { kind: "none" };
  // Every name given the same (a speaker with few lines): that one.
  if (agree === given.length) return decide(best);
  return { kind: "mixed", move: given };
}

/** One call to name a speaker: the whole speaker (`line` null) or one line. */
export interface NameStep {
  name: string;
  line: { start_ms: number; end_ms: number } | null;
}

/**
 * What to give, in order. The single lines first, while the speaker is still
 * the one asked about (named throughout with another speaker's name, it is
 * joined to that one and its number goes).
 */
export function namingSteps<T extends { start_ms: number; end_ms: number }>(
  v: Verdict<T>,
): NameStep[] {
  if (v.kind !== "decide" && v.kind !== "mixed") return [];
  const lines = v.move.map((a) => ({
    name: a.name ?? "",
    line: { start_ms: a.line.start_ms, end_ms: a.line.end_ms },
  }));
  return v.kind === "decide" ? [...lines, { name: v.name, line: null }] : lines;
}

/** Whether a Korean word ends in a final consonant (and which: 8 is ㄹ). */
function finalOf(word: string): number {
  const last = word.trim().slice(-1);
  // Digits as read in Korean: 0 영, 1 일, 3 삼, 6 육, 7 칠, 8 팔.
  const digits: Record<string, number> = {
    "0": 21,
    "1": 8,
    "3": 16,
    "6": 1,
    "7": 8,
    "8": 8,
  };
  if (/[0-9]/.test(last)) return digits[last] ?? 0;
  const code = last.charCodeAt(0) - 0xac00;
  return code < 0 || code > 11171 ? 0 : code % 28;
}

/** A name with 로/으로 ("레이로", "제제로", "회장님으로"). */
export function withRo(name: string): string {
  const f = finalOf(name);
  return `${name}${f === 0 || f === 8 ? "로" : "으로"}`;
}

/** A name with 예요/이에요 ("레이예요", "회장님이에요"). */
export function withIs(name: string): string {
  return `${name}${finalOf(name) === 0 ? "예요" : "이에요"}`;
}

/** A speaker who said less than this share (percent) is asked about last. */
export const QUIET_SHARE = 1;

/**
 * Who is still to be asked about, in order: those who said most first, the
 * quiet ones last; the unnamed only, or everyone. Speakers already seen this
 * round (named or skipped) are left out, and so is one wearing a name given
 * this round (lines moved to a person make a speaker of that name).
 */
export function speakerQueue<S extends { id: number; name: string }>(
  speakers: S[],
  share: Record<number, number>,
  who: "unnamed" | "all",
  unnamed: (name: string) => boolean,
  seen: number[],
  given: string[],
): S[] {
  const talked = speakers
    .filter((s) => (share[s.id] ?? 0) > 0)
    .sort((a, b) => (share[b.id] ?? 0) - (share[a.id] ?? 0));
  const asked = who === "all" ? talked : talked.filter((s) => unnamed(s.name));
  const loud = asked.filter((s) => (share[s.id] ?? 0) >= QUIET_SHARE);
  const quiet = asked.filter((s) => (share[s.id] ?? 0) < QUIET_SHARE);
  return [...loud, ...quiet].filter(
    (s) => !seen.includes(s.id) && !given.includes(s.name),
  );
}
