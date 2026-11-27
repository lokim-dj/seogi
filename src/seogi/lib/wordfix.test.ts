// bun src/seogi/lib/wordfix.test.ts

import assert from "node:assert/strict";
import type { Segment, Word } from "@/bindings";
import {
  distinctFixes,
  pieces,
  quotedRo,
  quotedTopic,
  suggestions,
} from "./wordfix";

const line: Segment = {
  start_ms: 0,
  end_ms: 1000,
  speaker: 1,
  text: "그 우물천장탄은 낮아요",
  edited: "그 우물천장은 낮아요",
  fixes: [{ at: 2, from: "우물천장탄", to: "우물천장", auto: false }],
};

// The line as read, the fixed word its own piece.
assert.deepEqual(pieces(line), [
  { text: "그 " },
  {
    text: "우물천장",
    fix: { at: 2, from: "우물천장탄", to: "우물천장", auto: false },
  },
  { text: "은 낮아요" },
]);
assert.equal(
  pieces(line)
    .map((piece) => piece.text)
    .join(""),
  line.edited,
);
// Words picked in unchanged words are cut out; a fixed word picked whole is
// marked as it is.
assert.deepEqual(
  pieces(line, { at: 8, n: 2 }).map((p) => [p.text, Boolean(p.picked)]),
  [
    ["그 ", false],
    ["우물천장", false],
    ["은 ", false],
    ["낮아", true],
    ["요", false],
  ],
);
assert.equal(pieces(line, { at: 2, n: 4 })[1].picked, true);
// A line without fixes is its words.
assert.deepEqual(pieces({ ...line, edited: null, fixes: [] }), [
  { text: line.text },
]);
assert.equal(distinctFixes([line, line]).length, 1);
// The vocabulary's fix is in the words as transcribed already.
assert.deepEqual(
  pieces({
    ...line,
    text: "중문은 3연동 슬라이딩",
    edited: null,
    fixes: [{ at: 4, from: "삼연동", to: "3연동", auto: true }],
  }).map((p) => p.text),
  ["중문은 ", "3연동", " 슬라이딩"],
);

// The vocabulary's look-alike words first; typing narrows them.
const words: Word[] = ["우물천장", "간접등", "우물", "아트월"].map((term) => ({
  term,
  misheard: [],
  editable: true,
}));
assert.equal(suggestions("우물천장탄", "", words)[0], "우물천장");
assert.deepEqual(suggestions("우물천장탄", "간접", words), ["간접등"]);
assert.equal(quotedRo("우물천장"), "「우물천장」으로");
assert.equal(quotedRo("간접등"), "「간접등」으로");
assert.equal(quotedRo("아트월"), "「아트월」로");

assert.equal(quotedTopic("우물 천정"), "「우물 천정」은");
assert.equal(quotedTopic("방마로"), "「방마로」는");

console.log("wordfix: ok");
