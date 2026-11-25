// bun src/seogi/lib/speakers.test.ts

import assert from "node:assert/strict";
import { speakerTones } from "./speakers";

// The first four speakers by number, in order, whatever order they speak in.
const four = speakerTones([3, 1, 2, 4], [2, 1, 3, 4]);
assert.deepEqual([...four.entries()].sort(), [
  [1, 1],
  [2, 2],
  [3, 3],
  [4, 4],
]);

// A fifth takes the tone of the one it never talks next to.
const five = speakerTones([0, 1, 2, 3, 4], [0, 4, 1, 4, 2, 4, 0, 1, 3]);
assert.equal(five.get(4), 4);
// Its neighbours' lines never share its tone.
const order = [0, 4, 1, 4, 2, 4];
for (let i = 1; i < order.length; i++) {
  assert.notEqual(five.get(order[i]), five.get(order[i - 1]));
}

// Without the fifth's lines, it goes round to the first tone.
assert.equal(speakerTones([0, 1, 2, 3, 4], []).get(4), 1);

console.log("speakers: ok");
