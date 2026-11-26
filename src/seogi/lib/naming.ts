// Naming who spoke from a few of their lines, one speaker at a time: a line
// is heard and a name chosen ("모르겠어요" counts for nothing), up to five;
// when four say the same name the whole speaker is that person, and the
// lines given another name go to that one alone.

import type { Naming } from "@/bindings";
import { sampleLines, sharedName, type SpokenLine } from "./speakers";

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
  /** The speaker's present name: when the answers go different ways, a
   *  line answered with it stays put. */
  current: string;
  /** Names already written down (the people, the speakers): a name given
   *  as one of these, with or without "님", is written the same way. */
  known: string[];
}

export function startAsking<T>(
  lines: T[],
  current = "",
  known: string[] = [],
): Asking<T> {
  return { lines, answers: [], more: false, current: current.trim(), known };
}

/** A name as compared: spaces tidied, "님" at the end and case ignored, so
 *  "레이" and "레이님" are one person. */
export function nameKey(name: string): string {
  return name
    .trim()
    .replace(/\s+/g, " ")
    .replace(/\s*님$/, "")
    .toLowerCase();
}

/** `name` as the known one written the same way, else as typed. */
export function knownAs(name: string, known: string[]): string {
  const key = nameKey(name);
  return known.find((k) => k.trim() && nameKey(k) === key)?.trim() ?? name;
}

/** The line now asked about gets `name` (null: "모르겠어요"). */
export function answer<T>(state: Asking<T>, name: string | null): Asking<T> {
  const line = state.lines[state.answers.length];
  if (line === undefined) return state;
  const typed = name?.trim().replace(/\s+/g, " ") || null;
  // Written as a name given before, here or among the known ones.
  const tidy = typed
    ? knownAs(typed, [
        ...state.known,
        ...state.answers.map((a) => a.name ?? ""),
      ])
    : null;
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
  for (const a of given) {
    const key = nameKey(a.name);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  const count = (name: string) => counts.get(nameKey(name)) ?? 0;
  // The name given most; on a tie, the one given first.
  let best: string | null = null;
  for (const a of given) {
    if (best === null || count(a.name) > count(best)) best = a.name;
  }
  const agree = best ? count(best) : 0;
  // Every line answered with another name goes to that one, also one
  // answered with the speaker's present name: the speaker is renamed.
  const decide = (name: string): Verdict<T> => ({
    kind: "decide",
    name,
    agree,
    of: given.length,
    move: given.filter((a) => nameKey(a.name) !== nameKey(name)),
  });
  if (best && agree >= SAME_ENOUGH) return decide(best);
  if (given.length < ASK_LINES && left > 0) return { kind: "asking" };
  // Five answered, or nothing more to hear.
  if (!best) return { kind: "none" };
  // Every name given the same (a speaker with few lines): that one.
  if (agree === given.length) return decide(best);
  // Two or more people under one number, which keeps its name: the lines
  // answered with that name stay.
  return {
    kind: "mixed",
    move: given.filter(
      (a) => !state.current || nameKey(a.name) !== nameKey(state.current),
    ),
  };
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

/** Whether a Korean word ends in a final consonant (and which: 8 is ㄹ);
 *  null when it does not end in Hangul or a digit (an English name). */
function finalOf(word: string): number | null {
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
  return code < 0 || code > 11171 ? null : code % 28;
}

/** A name with 로/으로 ("레이로", "회장님으로"; "Alex(으)로"). */
export function withRo(name: string): string {
  const f = finalOf(name);
  if (f === null) return `${name}(으)로`;
  return `${name}${f === 0 || f === 8 ? "로" : "으로"}`;
}

/** A name with 예요/이에요 ("레이예요", "회장님이에요"; "Alex(이)에요"). */
export function withIs(name: string): string {
  const f = finalOf(name);
  if (f === null) return `${name}(이)에요`;
  return `${name}${f === 0 ? "예요" : "이에요"}`;
}

/** A name in quotes with 와/과 ("'레이'와", "'회장님'과"; "'Alex'(와)과"). */
export function quotedWithWa(name: string): string {
  const f = finalOf(name);
  const particle = f === null ? "(와)과" : f === 0 ? "와" : "과";
  return `'${name}'${particle}`;
}

/**
 * The other speaker that speaker `id` becomes one with when named `name`
 * throughout, as the backend does it: one already called exactly that,
 * unless two people can share the name. Null when none.
 */
export function joinsWith<S extends { id: number; name: string }>(
  id: number,
  name: string,
  speakers: S[],
): S | null {
  const wanted = name.trim();
  if (sharedName(wanted)) return null;
  return speakers.find((s) => s.id !== id && s.name === wanted) ?? null;
}

/**
 * Who said what and who was given each task, kept just before a naming so
 * it can be put back (every line in order; every task's owner, minutes by
 * minutes, section by section).
 */
export function keptNaming(
  transcript: {
    speakers: { id: number; name: string }[];
    segments: { start_ms: number; speaker: number }[];
  },
  minutes: { sections: { entries: { owner: string }[] }[] }[],
): Naming {
  return {
    speakers: transcript.speakers.map((s) => ({ id: s.id, name: s.name })),
    lines: transcript.segments.map((line) => ({
      start_ms: line.start_ms,
      speaker: line.speaker,
    })),
    owners: minutes.flatMap((m) =>
      m.sections.flatMap((section) => section.entries.map((e) => e.owner)),
    ),
  };
}

/** A speaker who said less than this share (percent) is asked about last. */
export const QUIET_SHARE = 1;

/**
 * Who is still to be asked about, in order: those who said most first, the
 * quiet ones last; the unnamed only, or everyone. Speakers already seen this
 * round (named or skipped) are left out, and so is one that was not there
 * when the round began (`from`, the speakers then; null: no round), made by
 * moving a line to a person.
 */
export function speakerQueue<S extends { id: number; name: string }>(
  speakers: S[],
  share: Record<number, number>,
  who: "unnamed" | "all",
  unnamed: (name: string) => boolean,
  seen: number[],
  from: number[] | null,
): S[] {
  const talked = speakers
    .filter((s) => (share[s.id] ?? 0) > 0)
    .sort((a, b) => (share[b.id] ?? 0) - (share[a.id] ?? 0));
  const asked = who === "all" ? talked : talked.filter((s) => unnamed(s.name));
  const loud = asked.filter((s) => (share[s.id] ?? 0) >= QUIET_SHARE);
  const quiet = asked.filter((s) => (share[s.id] ?? 0) < QUIET_SHARE);
  return [...loud, ...quiet].filter(
    (s) => !seen.includes(s.id) && (from === null || from.includes(s.id)),
  );
}
