// Which calendar events the home puts beside the record key.

import type { Upcoming } from "@/bindings";

/** An event going on this long is likely over, or not one to record now. */
export const LONG_GOING_MS = 30 * 60_000;
/** A coming event this close is the one to get ready for. */
export const SOON_MS = 2 * 3_600_000;
/** How many more of today's events are listed small under the first. */
export const MORE_SHOWN = 3;

/**
 * The event going on now that the record key names, or null: of the events
 * not turned off that have begun and not yet ended, the one that began
 * last, as the backend picks for a recording started now.
 */
export function goingNow(today: Upcoming[], now: number): Upcoming | null {
  let found: Upcoming | null = null;
  for (const upcoming of today) {
    const { start_ms, end_ms } = upcoming.event;
    if (upcoming.muted || start_ms > now || end_ms <= now) continue;
    if (!found || start_ms > found.event.start_ms) found = upcoming;
  }
  return found;
}

/** The first event not turned off that is still to begin, on any day. */
export function nextAfter(today: Upcoming[], now: number): Upcoming | null {
  return (
    today.find(
      (upcoming) => !upcoming.muted && upcoming.event.start_ms > now,
    ) ?? null
  );
}

export type Shown = {
  /** The event shown large, the one a press records. */
  main: Upcoming | null;
  /** Today's other events, earliest first, at most [MORE_SHOWN]. */
  rest: Upcoming[];
  /** How many more of today's events there are beyond `rest`. */
  hidden: number;
};

/**
 * The first event that is not turned off leads; but when that one began
 * more than half an hour ago and another starts within two hours, the
 * coming one leads and the going one is listed small. While recording, the
 * going one stays first. Under it, the rest of today's events.
 */
export function arrange(
  today: Upcoming[],
  now: number,
  recording: boolean,
): Shown {
  const wanted = today.filter((upcoming) => !upcoming.muted);
  let main = wanted[0] ?? null;
  if (main && !recording && main.event.start_ms <= now) {
    const going = main;
    const coming = wanted.find((upcoming) => upcoming.event.start_ms > now);
    if (
      coming &&
      now - going.event.start_ms > LONG_GOING_MS &&
      coming.event.start_ms - now <= SOON_MS
    ) {
      main = coming;
    }
  }
  const midnight = new Date(now);
  midnight.setHours(24, 0, 0, 0);
  const others = wanted.filter(
    (upcoming) =>
      upcoming !== main && upcoming.event.start_ms < midnight.getTime(),
  );
  return {
    main,
    rest: others.slice(0, MORE_SHOWN),
    hidden: Math.max(0, others.length - MORE_SHOWN),
  };
}
