// bun src/seogi/lib/speakers.test.ts

import assert from "node:assert/strict";
import {
  maybeSamePerson,
  sampleLines,
  shareBar,
  speakerTones,
} from "./speakers";

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

// Three lines for a speaker: long enough, one from each part of the
// meeting, the surest of each part.
const line = (start: number, text: string, confidence = 0.9) => ({
  start_ms: start,
  end_ms: start + 3000,
  text,
  confidence,
});
const said = [
  line(1000, "네"),
  line(5000, "거실 천장은 낮아 보이지 않을까요", 0.7),
  line(9000, "현관 중문은 꼭 달고 싶어요", 0.95),
  line(20000, "예산은 사천오백 안쪽이에요"),
  line(25000, "아"),
  line(41000, "바닥은 마루로 해 주세요"),
  line(50000, "토요일에 같이 고를게요", 0.6),
];
assert.deepEqual(
  sampleLines(said, 60000).map((l) => l.start_ms),
  [9000, 20000, 41000],
);
// Talking only at the start: the best long lines from there, never "네".
const early = said.slice(0, 3);
assert.deepEqual(
  sampleLines(early, 60000).map((l) => l.start_ms),
  [1000, 5000, 9000],
);
assert.deepEqual(
  sampleLines(
    [line(1000, "네"), line(5000, "현관 중문은 꼭 달고 싶어요")],
    60000,
  ).map((l) => l.start_ms),
  [1000, 5000],
);
// Fewer lines than three: as many as there are.
assert.equal(sampleLines([line(0, "안녕하세요 반갑습니다")], 60000).length, 1);
assert.equal(sampleLines([], 60000).length, 0);

// Whole percentages that add up to 100, the most talking first.
const bar = shareBar([
  { speaker: 2, talk_ms: 1000 },
  { speaker: 1, talk_ms: 1000 },
  { speaker: 3, talk_ms: 1000 },
  { speaker: 4, talk_ms: 0 },
]);
assert.equal(bar.length, 3);
assert.equal(
  bar.reduce((n, p) => n + p.percent, 0),
  100,
);
const uneven = shareBar([
  { speaker: 1, talk_ms: 37 * 60000 },
  { speaker: 2, talk_ms: 26 * 60000 },
  { speaker: 3, talk_ms: 8 * 60000 },
]);
assert.deepEqual(
  uneven.map((p) => [p.speaker, p.percent]),
  [
    [1, 52],
    [2, 37],
    [3, 11],
  ],
);
assert.deepEqual(shareBar([]), []);

// The designer heard as speakers 1 and 3: each talks with the customer (2),
// never with each other. The customer answers both, so is no match.
{
  const named = [
    { id: 1, name: "김서윤 실장" },
    { id: 2, name: "고객" },
    { id: 3, name: "화자 3" },
  ];
  const unnamed = (name: string) => /^화자 \d+$/.test(name);
  const order = [1, 2, 1, 2, 1, 2, 3, 2, 3, 2, 3, 2, 3, 2];
  assert.equal(maybeSamePerson(3, named, order, unnamed), 1);
  // Two who answer each other are two people.
  const talking = [1, 3, 1, 3, 2, 1, 3, 2, 3, 1, 2];
  assert.equal(maybeSamePerson(3, named, talking, unnamed), null);
  // A role is no one to join.
  const roles = named.map((s) => (s.id === 1 ? { id: 1, name: "고객" } : s));
  assert.equal(maybeSamePerson(3, roles, order, unnamed), null);
  // Too little said to tell.
  assert.equal(maybeSamePerson(3, named, [1, 2, 3, 2], unnamed), null);
}

console.log("speakers: ok");
