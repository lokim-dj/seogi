// The transcript polished for reading (#108), as the minutes screen shows
// it. The rule is the backend's (`Polish::of` in polish.rs): a line shows
// its polished words only while nobody fixed it and it is still the words
// that were polished.

import type { Meeting, Segment } from "@/bindings";

/** The polished words of each line that has them, by the line's start. */
export function polishedLines(meeting: Meeting): Map<number, string> {
  const out = new Map<number, string>();
  const lines = meeting.polish?.lines ?? [];
  if (lines.length === 0) return out;
  const byStart = new Map(lines.map((line) => [line.start_ms, line]));
  for (const segment of meeting.transcript?.segments ?? []) {
    if (segment.edited !== undefined && segment.edited !== null) continue;
    const line = byStart.get(segment.start_ms);
    if (line && line.from === segment.text)
      out.set(segment.start_ms, line.text);
  }
  return out;
}

/** The lines to read: each polished one in its polished words. A polished
 *  line has no word fixes to show (the polish kept the vocabulary's words
 *  but not where they were). */
export function withPolish(
  segments: Segment[],
  polished: Map<number, string>,
): Segment[] {
  if (polished.size === 0) return segments;
  return segments.map((segment) => {
    const text = polished.get(segment.start_ms);
    return text === undefined ? segment : { ...segment, text, fixes: [] };
  });
}
