// The words a search found, in a line of text: where they are, and a short
// stretch around them for the list of meetings. Korean letters saved apart
// (NFD) and typed syllables (NFC) are the same words.

/** `text` in pieces, the ones that are `words` (any case) marked `hit`.
 *  Both are put into syllables first, so the pieces are of that text. */
export function pieces(
  text: string,
  words: string,
): { text: string; hit: boolean }[] {
  const line = text.normalize("NFC");
  const wanted = words.trim().normalize("NFC");
  if (!wanted) return [{ text: line, hit: false }];
  const escaped = wanted.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return line
    .split(new RegExp(`(${escaped})`, "iu"))
    .map((piece, index) => ({ text: piece, hit: index % 2 === 1 }))
    .filter((piece) => piece.text !== "");
}

/** Whether `words` are in `text`, in any case. */
export function has(text: string, words: string): boolean {
  return pieces(text, words).some((piece) => piece.hit);
}

/** A mark that belongs to the character before it: a joiner, a variation
 *  or skin-tone selector, a combining mark. A cut never falls before one. */
const CLINGS = /^[‍︎️\u{1f3fb}-\u{1f3ff}\u{e0020}-\u{e007f}\p{M}]$/u;

/** Whether a cut between `chars[at - 1]` and `chars[at]` splits a
 *  character as it is seen (an emoji made of several, a letter and its
 *  accent). */
function splits(chars: string[], at: number): boolean {
  if (at <= 0 || at >= chars.length) return false;
  return CLINGS.test(chars[at]) || chars[at - 1] === "‍";
}

/** A short stretch of `text` around the first place `words` are, counted
 *  in characters, with an ellipsis where it was cut. */
export function around(
  text: string,
  words: string,
  before = 16,
  after = 44,
): string {
  const line = text.normalize("NFC");
  const chars = Array.from(line);
  const parts = pieces(line, words);
  const first = parts.findIndex((part) => part.hit);
  let start: number;
  let end: number;
  if (first < 0) {
    start = 0;
    end = Math.min(chars.length, before + after);
  } else {
    const at = parts
      .slice(0, first)
      .reduce((n, part) => n + Array.from(part.text).length, 0);
    const length = Array.from(parts[first].text).length;
    start = Math.max(0, at - before);
    // Start at a word, when there is a space to start from.
    if (start > 0) {
      const space = chars.indexOf(" ", start);
      if (space >= 0 && space < at) start = space + 1;
    }
    end = Math.min(chars.length, at + length + after);
  }
  while (splits(chars, start)) start -= 1;
  while (splits(chars, end)) end += 1;
  const cut = end < chars.length;
  const stretch = chars.slice(start, end).join("");
  return `${start > 0 ? "…" : ""}${cut ? stretch.trimEnd() : stretch}${
    cut ? "…" : ""
  }`;
}
