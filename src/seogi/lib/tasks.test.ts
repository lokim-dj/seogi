// Run with: bun src/seogi/lib/tasks.test.ts
//
// The cases are shared with the Rust side (render.rs reads the same file),
// so the screen and 회의록.md tidy a task the same way.

import assert from "node:assert/strict";
import cases from "./tasks.cases.json";
import { isSpeakerLabel, sideOf, taskLine, titleNeeded } from "./tasks";

for (const { title, side } of cases.titles) {
  assert.equal(sideOf({ title }), side, title);
}

for (const line of cases.lines) {
  assert.deepEqual(
    taskLine({ title: line.title }, line, cases.names),
    { who: line.who, text: line.out, unowned: line.unowned },
    `${line.title} / ${line.owner} / ${line.text}`,
  );
}

for (const label of ["화자 2", "화자 A", "화자 2 (디자이너)", "화자대표"]) {
  assert.equal(isSpeakerLabel(label), true, label);
}
assert.equal(isSpeakerLabel("김서윤"), false);

assert.equal(titleNeeded({ title: "우리 숙제" }, 1), true);
assert.equal(titleNeeded({ title: "할 일" }, 1), false);
assert.equal(titleNeeded({ title: "할 일" }, 2), true);

console.log(`tasks: ${cases.lines.length} lines, all assertions passed`);
