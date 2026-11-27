// Misheard words fixed one at a time (src-tauri/src/seogi/transcript.rs,
// `WordFix`): a line's words as read, cut into the words as transcribed
// and the fixed ones, and the vocabulary's words to offer for a fix.

import type { Segment, Word, WordFix } from "@/bindings";
import { withRo, withTopic } from "./naming";

/** A run of a line as read: unchanged words, a fixed word (with what was
 *  heard there), or the words picked for fixing. */
export interface Piece {
  text: string;
  fix?: WordFix;
  picked?: boolean;
}

/** Characters as the backend counts them (Unicode scalar values). */
export const chars = (text: string) => Array.from(text);

/** The words of a line fixed word by word, not rewritten whole. */
export const fixedByWord = (line: Segment) => (line.fixes ?? []).length > 0;

/** How many characters of the line as transcribed a fix covers: the heard
 *  words for the user's fix, the right ones for the vocabulary's (which the
 *  line as transcribed has already). */
export const writtenLength = (fix: WordFix) =>
  chars(fix.auto ? fix.to : fix.from).length;

/** A line as read, in pieces: its words as transcribed with each fix put
 *  in. A line rewritten whole is one piece of its words. `pick` (characters
 *  from `at`, of the line as read) is cut out as its own piece when it lies
 *  in unchanged words or is exactly a fixed word. */
export function pieces(
  line: Segment,
  pick?: { at: number; n: number } | null,
): Piece[] {
  const out: Piece[] = [];
  const fixes = [...(line.fixes ?? [])].sort((a, b) => a.at - b.at);
  if (fixes.length === 0) {
    out.push({ text: line.edited ?? line.text });
  } else {
    const text = chars(line.text);
    let next = 0;
    for (const fix of fixes) {
      if (fix.at > next) out.push({ text: text.slice(next, fix.at).join("") });
      out.push({ text: fix.to, fix });
      next = fix.at + writtenLength(fix);
    }
    if (next < text.length) out.push({ text: text.slice(next).join("") });
  }
  if (!pick) return out;
  // Cut the picked characters out of the piece they lie in.
  const cut: Piece[] = [];
  let at = 0;
  for (const piece of out) {
    const length = chars(piece.text).length;
    const start = pick.at - at;
    const end = start + pick.n;
    if (piece.fix) {
      cut.push(
        start === 0 && end === length ? { ...piece, picked: true } : piece,
      );
    } else if (start >= 0 && end <= length && pick.n > 0) {
      const letters = chars(piece.text);
      if (start > 0) cut.push({ text: letters.slice(0, start).join("") });
      cut.push({ text: letters.slice(start, end).join(""), picked: true });
      if (end < length) cut.push({ text: letters.slice(end).join("") });
    } else {
      cut.push(piece);
    }
    at += length;
  }
  return cut;
}

/** The different fixes in some lines, each once: one mark per fix in a
 *  paragraph. */
export function distinctFixes(lines: Segment[]): WordFix[] {
  const seen = new Map<string, WordFix>();
  for (const line of lines) {
    for (const fix of line.fixes ?? []) {
      const key = `${fix.from}\u0000${fix.to}`;
      if (!seen.has(key)) seen.set(key, fix);
    }
  }
  return [...seen.values()];
}

/** Letter pairs of a word, for telling how alike two words look. */
function pairs(word: string): string[] {
  const letters = chars(word.replace(/\s+/g, ""));
  if (letters.length < 2) return letters;
  return letters.slice(1).map((letter, i) => letters[i] + letter);
}

/** How alike two words look, 0 to 1: the share of letter pairs they have
 *  in common. */
export function likeness(a: string, b: string): number {
  const left = pairs(a);
  const right = pairs(b);
  if (!left.length || !right.length) return 0;
  const rest = [...right];
  let shared = 0;
  for (const pair of left) {
    const at = rest.indexOf(pair);
    if (at >= 0) {
      shared += 1;
      rest.splice(at, 1);
    }
  }
  return (2 * shared) / (left.length + right.length);
}

/** The vocabulary's words to offer for the misheard `heard`: with nothing
 *  typed, the ones that look most like it; while typing, the ones with what
 *  is typed in them, the likest first. At most `limit`. */
export function suggestions(
  heard: string,
  typed: string,
  words: Word[],
  limit = 4,
): string[] {
  const query = typed.trim();
  const terms = words
    .map((word) => word.term)
    .filter((term) => term !== heard && term !== query);
  const ranked = terms
    .filter((term) => (query ? term.includes(query) : true))
    .map((term) => ({ term, score: likeness(heard, term) }))
    .filter(({ score }) => query || score >= 0.3)
    .sort((a, b) => b.score - a.score);
  return ranked.slice(0, limit).map(({ term }) => term);
}

/** Whether `word` is a word of the vocabulary as it is written right. */
export const knownWord = (word: string, words: Word[]) =>
  words.some((one) => one.term === word.trim());

/** A word in 「」 with 은/는 after it, for the Korean lines. */
export const quotedTopic = (word: string) =>
  `「${word}」${withTopic(word).slice(word.length)}`;

/** A word in 「」 with 로/으로 after it, for the Korean lines. */
export const quotedRo = (word: string) =>
  `「${word}」${withRo(word).slice(word.length)}`;
