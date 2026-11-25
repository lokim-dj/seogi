// bun src/seogi/lib/upcoming.test.ts

import assert from "node:assert/strict";
import type { Upcoming } from "@/bindings";
import { pick } from "./upcoming";

const MIN = 60_000;
const now = 16 * 60 * MIN + 43 * MIN;
const at = (uid: string, start: number, muted = false): Upcoming => ({
  event: {
    uid,
    title: uid,
    start_ms: start,
    end_ms: start + 4 * 60 * MIN,
    location: "",
    attendees: [],
  },
  muted,
});

const longGoing = at("two", 14 * 60 * MIN);
const five = at("five", 17 * 60 * MIN);

// Going for hours, and the next one in 17 minutes: the next one leads.
assert.deepEqual(pick([longGoing, five], now, false), {
  main: five,
  going: longGoing,
});
// While recording, the going one stays.
assert.deepEqual(pick([longGoing, five], now, true), {
  main: longGoing,
  going: null,
});
// Just begun: it is the one to record.
const justBegun = at("now", now - 10 * MIN);
assert.deepEqual(pick([justBegun, five], now, false), {
  main: justBegun,
  going: null,
});
// The next one is far off: the going one stays.
const evening = at("late", now + 3 * 60 * MIN);
assert.deepEqual(pick([longGoing, evening], now, false), {
  main: longGoing,
  going: null,
});
// Turned off is skipped; nothing at all is nothing.
assert.deepEqual(pick([at("off", now - 90 * MIN, true), five], now, false), {
  main: five,
  going: null,
});
assert.deepEqual(pick([], now, false), { main: null, going: null });

console.log("upcoming: ok");
