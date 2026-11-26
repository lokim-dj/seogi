// What the minutes may be missing at the start: the recording began well
// after its calendar event did, or in the middle of talk. The same rules as
// `Meeting::late_minutes` (src-tauri/src/seogi/store.rs) and
// `facts::starts_mid_talk` (src-tauri/src/seogi/facts.rs); keep them alike.

import type { Meeting } from "@/bindings";

/** A recording may start this long after its event without having missed
 *  anything worth saying. */
const LATE_AFTER_MS = 3 * 60_000;

/** Whole minutes the recording started after its event began; 0 when there
 *  is no event, it began in time, or after the event was over. */
export function lateMinutes(
  meeting: Pick<Meeting, "event" | "started_at_ms">,
): number {
  const event = meeting.event;
  if (!event) return 0;
  const late = meeting.started_at_ms - event.start_ms;
  if (late < LATE_AFTER_MS || meeting.started_at_ms >= event.end_ms) return 0;
  return Math.floor(late / 60_000);
}

const OPENINGS = [
  "안녕하세요",
  "안녕하십니까",
  "반갑습니다",
  "처음뵙",
  "시작하겠습니다",
  "시작할게요",
  "시작하죠",
  "시작합시다",
  "오늘은",
  "오늘안건",
  "녹음",
];

const squeezed = (text: string) => text.replace(/\s/g, "");

/** Whether the recording seems to begin in the middle of talk: words from
 *  the first moment, a long first line, and nothing in the first minute and
 *  a half that opens a meeting. */
export function startsMidTalk(
  lines: { start_ms: number; text: string }[],
): boolean {
  const first = lines[0];
  if (!first) return false;
  if (first.start_ms > 1_500 || squeezed(first.text).length < 30) return false;
  return !lines
    .filter((line) => line.start_ms <= first.start_ms + 90_000)
    .some((line) =>
      OPENINGS.some((word) => squeezed(line.text).includes(word)),
    );
}
