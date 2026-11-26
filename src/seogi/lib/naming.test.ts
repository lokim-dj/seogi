// bun src/seogi/lib/naming.test.ts

import assert from "node:assert/strict";
import {
  answer,
  askLines,
  askMore,
  named,
  namingSteps,
  speakerQueue,
  startAsking,
  verdict,
  withIs,
  withRo,
} from "./naming";

const line = (start: number, text = "거실 바닥은 밝은 쪽으로 가고 싶어요") => ({
  start_ms: start,
  end_ms: start + 4000,
  text,
});
const seven = [0, 10, 20, 30, 40, 50, 60].map((m) => line(m * 60000));

// Five lines from across the meeting first, in the order said, then spares.
{
  const lines = askLines(seven, 70 * 60000);
  assert.equal(lines.length, 7);
  const first = lines.slice(0, 5).map((l) => l.start_ms);
  assert.deepEqual(
    first,
    [...first].sort((a, b) => a - b),
  );
  assert.ok(first[0] < 15 * 60000 && first[4] > 55 * 60000);
  assert.equal(new Set(lines).size, 7);
  // Fewer lines than five: as many as there are.
  assert.equal(askLines(seven.slice(0, 2), 70 * 60000).length, 2);
}

const ask = (names: (string | null)[], lines = seven) =>
  names.reduce((s, name) => answer(s, name), startAsking(lines));

// Four of five the same: the whole speaker is that person, and the one
// line given someone else goes to that one, before the speaker is named.
{
  const s = ask(["지우", "지우", "하린", "지우", "지우"]);
  const v = verdict(s);
  assert.equal(v.kind, "decide");
  if (v.kind !== "decide") throw new Error();
  assert.equal(v.name, "지우");
  assert.equal(v.agree, 4);
  assert.equal(v.of, 5);
  assert.deepEqual(
    namingSteps(v).map((step) => [step.name, step.line?.start_ms ?? null]),
    [
      ["하린", 20 * 60000],
      ["지우", null],
    ],
  );
}

// Four the same before the fifth: asked to settle at once.
{
  const s = ask(["지우", "지우", "지우", "지우"]);
  const v = verdict(s);
  assert.equal(v.kind, "decide");
  if (v.kind !== "decide") throw new Error();
  assert.equal(v.of, 4);
  assert.deepEqual(namingSteps(v), [{ name: "지우", line: null }]);
  // "하나 더 들어 볼게요": the next line is asked, then settled again.
  const more = askMore(s);
  assert.equal(verdict(more).kind, "asking");
  const after = answer(more, "하린");
  const v2 = verdict(after);
  assert.equal(v2.kind, "decide");
  if (v2.kind !== "decide") throw new Error();
  assert.equal(v2.move.length, 1);
}

// Three of four: not yet.
assert.equal(verdict(ask(["지우", "지우", "하린", "지우"])).kind, "asking");

// Split between people: two or more under one number; only the lines
// answered move, and the speaker keeps its number.
{
  const v = verdict(ask(["지우", "하린", "지우", "하린", "도윤"]));
  assert.equal(v.kind, "mixed");
  const steps = namingSteps(v);
  assert.equal(steps.length, 5);
  assert.ok(steps.every((step) => step.line !== null));
  assert.deepEqual(
    steps.map((step) => step.name),
    ["지우", "하린", "지우", "하린", "도윤"],
  );
}

// "모르겠어요" counts for nothing: the next line is asked instead, the count
// stays, and the line is never moved.
{
  const s = ask([null, "지우", null, "지우", "지우", "지우"]);
  assert.equal(named(s), 4);
  assert.equal(s.answers.length, 6);
  const v = verdict(s);
  assert.equal(v.kind, "decide");
  if (v.kind !== "decide") throw new Error();
  assert.equal(v.of, 4);
  assert.deepEqual(namingSteps(v), [{ name: "지우", line: null }]);
  // Nothing but "모르겠어요" until the lines run out: nothing is named.
  const none = ask([null, null, null, null, null, null, null]);
  assert.equal(verdict(none).kind, "none");
  assert.deepEqual(namingSteps(verdict(none)), []);
  // Still lines left: keep asking.
  assert.equal(verdict(ask([null, null])).kind, "asking");
}

// A speaker with two lines, both the same: that one, out of two.
{
  const v = verdict(ask(["하린", "하린"], seven.slice(0, 2)));
  assert.equal(v.kind, "decide");
  if (v.kind !== "decide") throw new Error();
  assert.equal(v.of, 2);
  // Two lines, two people: mixed.
  assert.equal(verdict(ask(["하린", "지우"], seven.slice(0, 2))).kind, "mixed");
}

// A line answered with the speaker's present name stays where it is (a
// second speaker of that name is never made for it).
{
  const s = ["지우", "하린", "하린", "하린", "하린"].reduce(
    (now, name) => answer(now, name),
    startAsking(seven, "지우"),
  );
  assert.deepEqual(namingSteps(verdict(s)), [{ name: "하린", line: null }]);
  const split = ["지우", "하린", "지우", "도윤", "하린"].reduce(
    (now, name) => answer(now, name),
    startAsking(seven, "지우"),
  );
  assert.deepEqual(
    namingSteps(verdict(split)).map((step) => step.name),
    ["하린", "도윤", "하린"],
  );
}

// No more lines than there are; a name is tidied.
{
  const s = ask([" 지우 ", "지우"], seven.slice(0, 2));
  assert.equal(answer(s, "지우"), s);
  assert.equal(s.answers[0].name, "지우");
  assert.equal(askMore(s).more, false);
}

// The queue: those who said most first, the quiet last; skipping moves on to
// the next without naming anyone; a name just given is not asked again.
{
  const speakers = [
    { id: 1, name: "화자 1" },
    { id: 2, name: "화자 2" },
    { id: 3, name: "화자 3" },
    { id: 4, name: "고객" },
    { id: 5, name: "화자 5" },
  ];
  const share = { 1: 20, 2: 0.5, 3: 33, 4: 24, 5: 0 };
  const unnamed = (name: string) => /^화자 \d+$/.test(name);
  const ids = (list: { id: number }[]) => list.map((s) => s.id);
  assert.deepEqual(
    ids(speakerQueue(speakers, share, "unnamed", unnamed, [], [])),
    [3, 1, 2],
  );
  assert.deepEqual(
    ids(speakerQueue(speakers, share, "all", unnamed, [], [])),
    [3, 4, 1, 2],
  );
  // 건너뛰기: speaker 3 is seen and left as it was.
  assert.deepEqual(
    ids(speakerQueue(speakers, share, "unnamed", unnamed, [3], [])),
    [1, 2],
  );
  // Lines given "고객" this round: that speaker is not asked about.
  assert.deepEqual(
    ids(speakerQueue(speakers, share, "all", unnamed, [3], ["고객"])),
    [1, 2],
  );
}

// Korean particles after a name.
assert.equal(withRo("지우"), "지우로");
assert.equal(withRo("회장님"), "회장님으로");
assert.equal(withRo("하늘"), "하늘로");
assert.equal(withIs("하린"), "하린이에요");
assert.equal(withRo("말한 사람 3"), "말한 사람 3으로");
assert.equal(withIs("지우"), "지우예요");
assert.equal(withIs("회장님"), "회장님이에요");

console.log("naming: ok");
