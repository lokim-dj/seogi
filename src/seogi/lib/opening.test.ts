// bun src/seogi/lib/opening.test.ts

import assert from "node:assert/strict";
import { lateMinutes, opens, startsMidTalk } from "./opening";

const MIN = 60_000;
const start = 1_000_000_000_000;
const event = {
  uid: "a",
  title: "상담",
  start_ms: start,
  end_ms: start + 60 * MIN,
};

assert.equal(lateMinutes({ event: null, started_at_ms: start + 8 * MIN }), 0);
assert.equal(lateMinutes({ event, started_at_ms: start + 8 * MIN + 5_000 }), 8);
assert.equal(lateMinutes({ event, started_at_ms: start + 2 * MIN }), 0);
assert.equal(lateMinutes({ event, started_at_ms: start + 61 * MIN }), 0);

assert.equal(
  startsMidTalk([
    {
      start_ms: 0,
      text: "그래서 이 벽을 이렇게 옮기면 거실이 훨씬 넓어 보이는 구조가 되는 거고요 수납은",
    },
    { start_ms: 10_000, text: "네" },
  ]),
  true,
);
assert.equal(
  startsMidTalk([
    {
      start_ms: 0,
      text: "안녕하세요 오늘은 거실 구조를 보겠습니다 먼저 도면부터 말씀드리면 이렇게",
    },
  ]),
  false,
);
assert.equal(
  startsMidTalk([
    {
      start_ms: 4_000,
      text: "아주 긴 첫 줄이지만 녹음이 시작되고 한참 뒤에 나온 말입니다 그러니까",
    },
  ]),
  false,
);

// An English greeting opens a meeting too.
assert.equal(
  startsMidTalk([
    {
      start_ms: 0,
      text: "Hi, nice to meet you. 영어 반 한국어 반으로 이야기해 볼게요 괜찮으시죠 주방부터 볼까요",
    },
    { start_ms: 10_000, text: "네" },
  ]),
  false,
);
assert.equal(opens("Good morning everyone"), true);
assert.equal(opens("Let's get started with the kitchen"), true);
assert.equal(opens("this is the higher cabinet"), false);
// A recording mostly not in Korean letters is not judged.
assert.equal(
  startsMidTalk([
    {
      start_ms: 0,
      text: "so if we move this wall the living room looks much wider and the storage goes here",
    },
    { start_ms: 10_000, text: "right, and the kitchen?" },
  ]),
  false,
);

console.log("opening: ok");
