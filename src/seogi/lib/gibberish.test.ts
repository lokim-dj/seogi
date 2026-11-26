// bun src/seogi/lib/gibberish.test.ts

import assert from "node:assert/strict";
import cases from "./gibberish.cases.json";
import { isGibberish } from "./gibberish";

for (const { text, gibberish } of cases) {
  assert.equal(isGibberish(text), gibberish, text);
}
console.log("gibberish: ok");
