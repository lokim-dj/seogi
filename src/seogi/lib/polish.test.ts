// bun src/seogi/lib/polish.test.ts

import assert from "node:assert/strict";
import type { Meeting, Segment } from "@/bindings";
import { polishedLines, withPolish } from "./polish";

const segments: Segment[] = [
  { start_ms: 0, end_ms: 1000, speaker: 1, text: "음 안녕하세요", fixes: [] },
  { start_ms: 2000, end_ms: 3000, speaker: 1, text: "자료 보내요", fixes: [] },
  {
    start_ms: 4000,
    end_ms: 5000,
    speaker: 2,
    text: "어 그건 안돼요",
    edited: "그건 안 돼요!",
    fixes: [],
  },
];

const meeting = {
  transcript: { language: "ko", model: "", segments, speakers: [] },
  polish: {
    lines: [
      { start_ms: 0, from: "음 안녕하세요", text: "안녕하세요." },
      // Polished from other words: transcribed again since.
      { start_ms: 2000, from: "자료 보낼게요", text: "자료 보낼게요." },
      // The user fixed this line: their words win.
      { start_ms: 4000, from: "어 그건 안돼요", text: "그건 안 돼요." },
    ],
  },
} as unknown as Meeting;

const polished = polishedLines(meeting);
assert.deepEqual([...polished], [[0, "안녕하세요."]]);

const read = withPolish(segments, polished);
assert.equal(read[0].text, "안녕하세요.");
assert.equal(read[1], segments[1]);
assert.equal(read[2], segments[2]);
// The transcript itself is not touched.
assert.equal(segments[0].text, "음 안녕하세요");

assert.equal(polishedLines({ ...meeting, polish: null }).size, 0);
console.log("polish: ok");
