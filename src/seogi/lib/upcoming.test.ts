// bun src/seogi/lib/upcoming.test.ts

import assert from "node:assert/strict";
import type { Upcoming } from "@/bindings";
import { arrange } from "./upcoming";

const MIN = 60_000;
const day = new Date(2026, 10, 25).getTime();
const now = day + 16 * 60 * MIN + 43 * MIN;
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

const longGoing = at("two", day + 14 * 60 * MIN);
const five = at("five", day + 17 * 60 * MIN);

// Going for hours, and the next one in 17 minutes: the next one leads.
assert.deepEqual(arrange([longGoing, five], now, false), {
  main: five,
  rest: [longGoing],
  hidden: 0,
});
// While recording, the going one stays first.
assert.deepEqual(arrange([longGoing, five], now, true), {
  main: longGoing,
  rest: [five],
  hidden: 0,
});
// Just begun: it is the one to record.
const justBegun = at("now", now - 10 * MIN);
assert.equal(arrange([justBegun, five], now, false).main, justBegun);
// The next one is far off: the going one stays.
const evening = at("late", now + 3 * 60 * MIN);
assert.equal(arrange([longGoing, evening], now, false).main, longGoing);
// Turned off is left out; nothing at all is nothing.
assert.deepEqual(arrange([at("off", now - 90 * MIN, true), five], now, false), {
  main: five,
  rest: [],
  hidden: 0,
});
assert.deepEqual(arrange([], now, false), { main: null, rest: [], hidden: 0 });
// Three more are listed, the others counted; tomorrow is not today's.
const many = [0, 1, 2, 3, 4].map((n) => at(`e${n}`, now + (20 + n * 30) * MIN));
const tomorrow = at("tomorrow", day + 33 * 60 * MIN);
assert.deepEqual(arrange([...many, tomorrow], now, false), {
  main: many[0],
  rest: many.slice(1, 4),
  hidden: 1,
});

console.log("upcoming: ok");
