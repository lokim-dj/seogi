// The words a search found, shown where they are: cut out of a long line
// for the list of meetings, and marked in the line itself on the minutes.

/** `text` in pieces, the ones that are `words` (any case) marked `hit`. */
function pieces(text: string, words: string): { text: string; hit: boolean }[] {
  const wanted = words.trim();
  if (!wanted) return [{ text, hit: false }];
  const escaped = wanted.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return text
    .split(new RegExp(`(${escaped})`, "i"))
    .map((piece, index) => ({ text: piece, hit: index % 2 === 1 }))
    .filter((piece) => piece.text !== "");
}

/** Whether `words` are in `text`, in any case. */
export function has(text: string, words: string): boolean {
  return pieces(text, words).some((piece) => piece.hit);
}

/** A short stretch of `text` around the first place `words` are, with an
 *  ellipsis where it was cut. */
export function around(text: string, words: string, before = 16, after = 44) {
  const lower = text.toLowerCase();
  const at = lower.indexOf(words.trim().toLowerCase());
  if (at < 0)
    return text.length > before + after
      ? `${text.slice(0, before + after)}…`
      : text;
  let start = Math.max(0, at - before);
  // Start at a word, when there is a space to start from.
  if (start > 0) {
    const space = text.indexOf(" ", start);
    if (space >= 0 && space < at) start = space + 1;
  }
  const end = Math.min(text.length, at + words.trim().length + after);
  return `${start > 0 ? "…" : ""}${text.slice(start, end)}${end < text.length ? "…" : ""}`;
}

/** `text` with every place `words` are marked; the whole of it when
 *  `wholeIfAbsent` and they are nowhere in it. */
export function Marked({
  text,
  words,
  className = "hl",
  wholeIfAbsent = false,
}: {
  text: string;
  words: string;
  className?: string;
  wholeIfAbsent?: boolean;
}) {
  const parts = pieces(text, words);
  if (!parts.some((part) => part.hit)) {
    return wholeIfAbsent && text ? (
      <mark className={className}>{text}</mark>
    ) : (
      <>{text}</>
    );
  }
  return (
    <>
      {parts.map((part, index) =>
        part.hit ? (
          <mark key={index} className={className}>
            {part.text}
          </mark>
        ) : (
          part.text
        ),
      )}
    </>
  );
}
