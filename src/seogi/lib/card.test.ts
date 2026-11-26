// bun src/seogi/lib/card.test.ts
//
// The one-page summary shows only what the minutes have: a part with
// nothing is left out, a doubted line is not set down as a fact, long
// words are cut, and no line to check means no red list.

import assert from "node:assert/strict";
import type { Meeting, MinutesSection, TalkStats } from "@/bindings";
import fixture from "../preview/fixtures/consult.json";
import {
  cardOf,
  fitSteps,
  LINE_MAX,
  roomAt,
  shorten,
  TITLE_MAX,
  type CardWords,
} from "./card";
import { A4_POINTS, jpegPdf } from "./pdf";

const words: CardWords = {
  untitled: "제목 없는 녹음",
  customer: (name) => `${name} 고객`,
  speaker: (name) => name.replace(/^화자 (\d+)$/, "말한 사람 $1"),
  reason: (reason) => `이유:${reason}`,
};

const base = () => structuredClone(fixture) as unknown as Meeting;
const talk: TalkStats = {
  speakers: [
    {
      speaker: 1,
      talk_ms: 60000,
      share: 50,
      turns: 1,
      longest_ms: 1,
      longest_at_ms: 0,
      questions: 0,
      cut_ins: 0,
    },
    {
      speaker: 2,
      talk_ms: 40000,
      share: 33,
      turns: 1,
      longest_ms: 1,
      longest_at_ms: 0,
      questions: 0,
      cut_ins: 0,
    },
    {
      speaker: 3,
      talk_ms: 20000,
      share: 17,
      turns: 1,
      longest_ms: 1,
      longest_at_ms: 0,
      questions: 0,
      cut_ins: 0,
    },
  ],
  moments: [],
};
const without = (meeting: Meeting, keep: (s: MinutesSection) => boolean) => {
  const minutes = meeting.minutes![0];
  minutes.sections = minutes.sections.filter(keep);
  return meeting;
};

// The whole made-up consultation: every part is there.
{
  const card = cardOf(base(), talk, "kakao", words);
  assert.equal(card.title, "이서진 고객");
  assert.equal(card.sub, "한빛아파트 1502호 상담");
  assert.equal(card.kind, "고객 상담");
  // Decisions: the doubted one (붙박이장 여닫이) is not among them.
  assert.ok(card.decisions);
  assert.equal(card.decisions.title, "결정 사항");
  assert.equal(card.decisions.lines.length, 4);
  assert.equal(card.decisions.more, 2);
  assert.ok(card.decisions.lines.every((line) => !/여닫이/.test(line.text)));
  assert.equal(card.decisions.lines[0].ms, 40716);
  // The budget drawn large, from the words as said.
  assert.ok(card.numbers?.lead);
  assert.equal(card.numbers.lead.value, "4,500");
  assert.equal(card.numbers.lead.unit, "만 원");
  assert.equal(card.numbers.lead.line.ms, 15340);
  // Other amounts, not from the decisions and not from what was left open.
  assert.ok(card.numbers.lines.length > 0);
  assert.ok(card.numbers.lines.every((line) => /\d/.test(line.text)));
  // Ours first, then the customer's.
  assert.deepEqual(
    card.tasks.map((group) => group.title),
    ["우리 숙제", "고객 숙제"],
  );
  assert.equal(card.tasks[0].lines.length, 3);
  assert.equal(card.tasks[0].more, 3);
  assert.match(card.tasks[0].lines[0].note, /김서윤 · 금요일까지/);
  // The one doubted line, with its reason and time.
  assert.equal(card.checkCount, 1);
  assert.equal(card.checks?.lines[0].ms, 148844);
  assert.equal(card.checks?.lines[0].note, "이유:unsure_speech");
  // The flow from the story parts' first moments; ticks inside the length.
  assert.ok(card.flow);
  assert.deepEqual(
    card.flow.parts.map((part) => [part.title, part.startMs, part.endMs]),
    [
      ["예산과 거실", 68020, 180716],
      ["현관과 중문", 180716, 201644],
      ["남은 것", 201644, 215875],
    ],
  );
  assert.ok(card.flow.ticks.some((tick) => tick.red && tick.ms === 148844));
  // Who talked, adding up to 100.
  assert.equal(card.share.length, 3);
  assert.equal(
    card.share.reduce((n, piece) => n + piece.percent, 0),
    100,
  );
  // The A4 page takes more lines.
  const a4 = cardOf(base(), talk, "a4", words);
  assert.equal(a4.decisions?.lines.length, 6);
  assert.equal(a4.tasks[0].lines.length, 6);
}

// No decisions and no numbers: those parts are gone, not drawn empty.
{
  const meeting = without(
    base(),
    (s) => s.kind === "actions" || s.key === "open" || s.key === "concerns",
  );
  const card = cardOf(meeting, talk, "kakao", words);
  assert.equal(card.decisions, null);
  assert.equal(card.numbers, null);
  assert.equal(card.tasks.length, 2);
}

// A decisions part whose every line is doubted shows nothing as decided.
{
  const meeting = base();
  for (const entry of meeting.minutes![0].sections.find(
    (s) => s.key === "decisions",
  )!.entries) {
    entry.reasons = ["unsure_speech"];
  }
  const card = cardOf(meeting, talk, "kakao", words);
  assert.equal(card.decisions, null);
  assert.equal(card.checkCount, 7);
  assert.equal(card.checks?.lines.length, 3);
  assert.equal(card.checks?.more, 4);
}

// A line the minutes show faintly is not set down as decided or as a number.
{
  const meeting = base();
  const sections = meeting.minutes![0].sections;
  sections.find((s) => s.key === "decisions")!.entries[0].faint = true;
  sections.find((s) => s.key === "budget")!.entries[0].faint = true;
  const card = cardOf(meeting, talk, "a4", words);
  assert.ok(card.decisions?.lines.every((line) => line.ms !== 40716));
  // Nor is another amount drawn large in the budget's place.
  assert.equal(card.numbers?.lead, null);
  assert.ok(card.numbers?.lines.every((line) => !/4500/.test(line.text)));
  assert.equal(card.checkCount, 1);
}

// Nothing to check: no red list, the count is 0.
{
  const meeting = base();
  for (const section of meeting.minutes![0].sections) {
    for (const entry of section.entries) entry.reasons = [];
  }
  const card = cardOf(meeting, talk, "kakao", words);
  assert.equal(card.checkCount, 0);
  assert.equal(card.checks, null);
  assert.ok(card.flow?.ticks.every((tick) => !tick.red));
}

// A long title is cut with "…"; long lines too.
{
  const meeting = base();
  meeting.customer = "";
  meeting.title =
    "한빛아파트 103동 1502호 거실 천장과 바닥 그리고 주방 상부장과 붙박이장까지 두 번째로 길게 이야기한 상담";
  const card = cardOf(meeting, talk, "kakao", words);
  assert.ok(Array.from(card.title).length <= TITLE_MAX);
  assert.ok(card.title.endsWith("…"));
  assert.equal(shorten("짧은 제목", TITLE_MAX), "짧은 제목");
  const long = "가".repeat(200);
  assert.equal(Array.from(shorten(long, LINE_MAX)).length, LINE_MAX);
}

// Without a title or a customer, the untitled name.
{
  const meeting = base();
  meeting.customer = "";
  meeting.title = "";
  assert.equal(cardOf(meeting, null, "kakao", words).title, "제목 없는 녹음");
}

// No story parts, or only one: no flow. No talk measured: no share.
{
  const meeting = base();
  meeting.minutes![0].story = meeting.minutes![0].story!.slice(0, 1);
  const card = cardOf(meeting, null, "kakao", words);
  assert.equal(card.flow, null);
  assert.deepEqual(card.share, []);
}

// No minutes at all: only the masthead.
{
  const meeting = base();
  meeting.minutes = [];
  const card = cardOf(meeting, null, "kakao", words);
  assert.equal(card.decisions, null);
  assert.equal(card.numbers, null);
  assert.deepEqual(card.tasks, []);
  assert.equal(card.flow, null);
  assert.equal(card.checks, null);
  assert.equal(card.kind, null);
}

// Stepping down takes a line from the fullest part, never below one.
{
  assert.deepEqual(roomAt("kakao", 0), {
    decisions: 4,
    numbers: 3,
    tasks: 3,
    checks: 3,
  });
  assert.deepEqual(roomAt("kakao", 1), {
    decisions: 3,
    numbers: 3,
    tasks: 3,
    checks: 3,
  });
  assert.deepEqual(roomAt("kakao", 2).tasks, 2);
  const least = roomAt("kakao", fitSteps("kakao") + 5);
  assert.deepEqual(least, { decisions: 1, numbers: 1, tasks: 1, checks: 1 });
  assert.equal(
    cardOf(base(), talk, "kakao", words, 99).decisions?.lines.length,
    1,
  );
}

// The PDF: one A4 page whose cross-reference points at each object.
{
  const jpeg = new Uint8Array([0xff, 0xd8, 0xff, 0xd9]);
  const pdf = jpegPdf(jpeg, 10, 14, A4_POINTS);
  const text = new TextDecoder("latin1").decode(pdf);
  assert.ok(text.startsWith("%PDF-1.4"));
  assert.ok(text.trimEnd().endsWith("%%EOF"));
  assert.match(text, /\/MediaBox \[0 0 595\.28 841\.89\]/);
  const xref = Number(/startxref\n(\d+)/.exec(text)![1]);
  assert.equal(text.slice(xref, xref + 4), "xref");
  const offsets = [...text.slice(xref).matchAll(/(\d{10}) 00000 n/g)].map(
    (match) => Number(match[1]),
  );
  assert.equal(offsets.length, 5);
  offsets.forEach((offset, index) =>
    assert.equal(
      text.slice(offset, offset + `${index + 1} 0 obj`.length),
      `${index + 1} 0 obj`,
    ),
  );
}

console.log("card: ok");
