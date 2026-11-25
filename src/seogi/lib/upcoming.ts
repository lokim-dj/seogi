// Which calendar event the home puts beside the record key.

import type { Upcoming } from "@/bindings";

/** An event going on this long is likely over, or not one to record now. */
export const LONG_GOING_MS = 30 * 60_000;
/** A coming event this close is the one to get ready for. */
export const SOON_MS = 2 * 3_600_000;

export type Shown = {
  /** The event shown large, the one a press records. */
  main: Upcoming | null;
  /** An event still going, shown small under a coming one. */
  going: Upcoming | null;
};

/**
 * The first event that is not turned off; but when that one began more than
 * half an hour ago and another starts within two hours, the coming one,
 * with the going one beside it. While recording, the going one stays.
 */
export function pick(
  today: Upcoming[],
  now: number,
  recording: boolean,
): Shown {
  const wanted = today.filter((upcoming) => !upcoming.muted);
  const first = wanted[0] ?? null;
  if (!first || recording || first.event.start_ms > now) {
    return { main: first, going: null };
  }
  const coming = wanted.find((upcoming) => upcoming.event.start_ms > now);
  if (
    coming &&
    now - first.event.start_ms > LONG_GOING_MS &&
    coming.event.start_ms - now <= SOON_MS
  ) {
    return { main: coming, going: first };
  }
  return { main: first, going: null };
}
