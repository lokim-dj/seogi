// bun src/seogi/lib/gibberish.test.ts

import assert from "node:assert/strict";
import cases from "./gibberish.cases.json";
import { isGibberish, koreanMeeting } from "./gibberish";

for (const { text, korean, gibberish } of cases) {
  assert.equal(isGibberish(text, korean), gibberish, text);
}
assert.equal(
  koreanMeeting([
    { text: "This is the final layout we agreed on." },
    { text: "Okay." },
  ]),
  false,
);
assert.equal(
  koreanMeeting([
    { text: "도면은 이렇게 바꾸겠습니다. 수납은 벽면 전체로 짜겠습니다." },
    { text: "네 좋아요 그렇게 해 주세요." },
    { text: "euch com" },
  ]),
  true,
);
console.log("gibberish: ok");
