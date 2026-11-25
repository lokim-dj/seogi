// bun src/seogi/lib/found.test.ts

import assert from "node:assert/strict";
import { around, has, pieces } from "./found";

const apart = (text: string) => text.normalize("NFD");

// Any case, and the pieces put back together are the line.
assert.deepEqual(pieces("Kick-off 회의, kick-OFF 다시", "kick-off"), [
  { text: "Kick-off", hit: true },
  { text: " 회의, ", hit: false },
  { text: "kick-OFF", hit: true },
  { text: " 다시", hit: false },
]);
assert.deepEqual(pieces("아무것도", "  "), [{ text: "아무것도", hit: false }]);
assert.equal(has("가격 (부가세 별도)", "(부가세"), true);

// Letters saved apart and syllables typed are the same words, both ways.
assert.equal(has(apart("이사 날짜를 다시 잡기"), "날짜"), true);
assert.equal(has("이사 날짜를 다시 잡기", apart("날짜")), true);
assert.deepEqual(
  pieces(apart("날짜"), "날짜").map((piece) => piece.text),
  ["날짜"],
);

// A long line is cut around the words, at a word, with ellipses.
const long =
  "처음에는 다른 이야기를 길게 하다가 마지막에 가서야 이사 날짜를 다음 달 둘째 주로 미루기로 하고 짐은 그 전에 맡기기로 했다";
assert.equal(
  around(long, "이사 날짜", 10, 10),
  "…마지막에 가서야 이사 날짜를 다음 달 둘째…",
);
assert.equal(around("짧은 줄", "줄"), "짧은 줄");

// A character as it is seen (an emoji of several, a skin tone) is never
// cut in half, at the start or at the end.
const family = "👨‍👩‍👧";
assert.equal(around(`가가${family}나나나`, "나", 1, 0), `…${family}나…`);
assert.equal(around(`나${family}가가`, "나", 0, 1), `나${family}…`);
assert.equal(around("가가👍🏽나나", "나", 1, 0), "…👍🏽나…");

console.log("found: ok");
