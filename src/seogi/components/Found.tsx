// The words a search found, marked in the line they are in.

import { pieces } from "../lib/found";

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
