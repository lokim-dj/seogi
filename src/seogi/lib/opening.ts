// What the minutes may be missing at the start: the recording began well
// after its calendar event did, or in the middle of talk. The same rules as
// `Meeting::late_minutes` (src-tauri/src/seogi/store.rs) and
// `facts::starts_mid_talk` (src-tauri/src/seogi/facts.rs); keep them alike.

import type { Meeting } from "@/bindings";
import { lettersHeard } from "./gibberish";

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

/** Openings in English, as whole words in lower case with marks dropped
 *  ("let's" is "lets"). */
const OPENINGS_IN_ENGLISH = [
  "hi",
  "hello",
  "hey",
  "good morning",
  "good afternoon",
  "good evening",
  "nice to meet you",
  "pleased to meet you",
  "welcome",
  "lets start",
  "lets get started",
  "lets begin",
];

/** The share of a recording that must be in Korean letters for its start to
 *  be judged. */
const MID_TALK_HANGUL_SHARE = 0.5;

const squeezed = (text: string) => text.replace(/\s/g, "");

/** Whether a line says something that opens a meeting. */
export function opens(text: string): boolean {
  const korean = squeezed(text);
  if (OPENINGS.some((word) => korean.includes(word))) return true;
  const words = ` ${text
    .replace(/['\u2019]/g, "")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim()} `;
  return OPENINGS_IN_ENGLISH.some((opening) => words.includes(` ${opening} `));
}

/** Whether the recording seems to begin in the middle of talk: words from
 *  the first moment, a long first line, and nothing in the first minute and
 *  a half that opens a meeting, in Korean or English. A recording mostly
 *  not in Korean letters is not judged. */
export function startsMidTalk(
  lines: { start_ms: number; text: string }[],
): boolean {
  const first = lines[0];
  if (!first) return false;
  if (first.start_ms > 1_500 || squeezed(first.text).length < 30) return false;
  const { letters, hangul } = lettersHeard(lines);
  if (hangul < MID_TALK_HANGUL_SHARE * letters) return false;
  return !lines
    .filter((line) => line.start_ms <= first.start_ms + 90_000)
    .some((line) => opens(line.text));
}
