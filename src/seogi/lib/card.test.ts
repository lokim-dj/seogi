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
  moneyOf,
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

// An amount is taken whole; two amounts or a range are not drawn large.
{
  assert.deepEqual(moneyOf("총 예산은 1억 2천만 원"), {
    value: "1억 2천",
    unit: "만 원",
  });
  assert.deepEqual(moneyOf("예산 3억 5000만원으로 확정"), {
    value: "3억 5,000",
    unit: "만 원",
  });
  assert.deepEqual(moneyOf("예산은 4500만원 안쪽"), {
    value: "4,500",
    unit: "만 원",
  });
  assert.deepEqual(moneyOf("예산 2억 원"), { value: "2", unit: "억 원" });
  assert.equal(moneyOf("예산은 4500~5000만원 사이"), null);
  assert.equal(moneyOf("예산을 5000만 원에서 4500만 원으로 줄임"), null);
  assert.equal(moneyOf("예산은 아직 모른다"), null);

  const said = (text: string) => {
    const meeting = base();
    const budget = meeting.minutes![0].sections.find(
      (s) => s.key === "budget",
    )!;
    budget.entries[0].text = text;
    return cardOf(meeting, talk, "a4", words).numbers;
  };
  assert.equal(said("총 예산은 1억 2천만 원이다.")?.lead?.value, "1억 2천");
  assert.equal(
    said("예산 3억 5000만원으로 확정했다.")?.lead?.value,
    "3억 5,000",
  );
  for (const text of [
    "예산은 4500~5000만원 사이로 생각한다.",
    "예산을 5000만 원에서 4500만 원으로 줄였다.",
  ]) {
    const numbers = said(text);
    assert.equal(numbers?.lead, null, text);
    // The line itself is still there, as said.
    assert.ok(
      numbers?.lines.some((line) => text.startsWith(line.text)),
      text,
    );
  }
}

// Headings that say "not settled" are not decisions, even with 결정 or 합의.
{
  for (const title of ["결정 보류", "결정할 것", "합의 안 된 것", "결정 전"]) {
    const meeting = base();
    meeting.minutes![0].sections.find((s) => s.key === "decisions")!.key =
      "mine";
    meeting.minutes![0].sections.find((s) => s.key === "mine")!.title = title;
    const card = cardOf(meeting, talk, "kakao", words);
    assert.equal(card.decisions, null, title);
  }
  for (const title of ["합의한 것", "약속한 것", "결정 사항"]) {
    const meeting = base();
    meeting.minutes![0].sections.find((s) => s.key === "decisions")!.key =
      "mine";
    meeting.minutes![0].sections.find((s) => s.key === "mine")!.title = title;
    assert.ok(cardOf(meeting, talk, "kakao", words).decisions, title);
  }
}

// Numbers come only from the parts that hold figures, not worries,
// wishes, risks or talk.
{
  const meeting = base();
  const sections = meeting.minutes![0].sections;
  sections.find((s) => s.key === "concerns")!.entries[0].text =
    "공사비가 5000만 원을 넘을까 걱정했다.";
  sections.find((s) => s.key === "requests")!.entries[0].text =
    "거실 폭 4200mm 를 넓히고 싶다고 했다.";
  sections.push({
    key: "risks",
    title: "리스크",
    kind: "items",
    entries: [
      {
        text: "자재비가 10% 오를 수 있다.",
        owner: "",
        due: "",
        sources: [{ ms: 1000, quote: "" }],
        reasons: [],
      },
    ],
  });
  const card = cardOf(meeting, talk, "a4", words);
  const all = [
    ...(card.numbers?.lines ?? []),
    ...(card.numbers?.lead ? [card.numbers.lead.line] : []),
  ].map((line) => line.text);
  assert.ok(all.length > 0);
  assert.ok(
    all.every((text) => !/걱정했다|넓히고|오를 수/.test(text)),
    all.join(),
  );
}

// A doubted task goes under 확인 필요; a faint one or one with no moment
// stays, marked.
{
  const meeting = base();
  const ours = meeting.minutes![0].sections.find((s) => s.key === "our_tasks")!;
  ours.entries[0].reasons = ["owner_unclear"];
  ours.entries[1].faint = true;
  ours.entries[2].sources = [];
  const card = cardOf(meeting, talk, "a4", words);
  const group = card.tasks[0];
  assert.equal(group.lines.length + group.more, 5);
  assert.ok(group.lines.every((line) => !/신발장/.test(line.text)));
  assert.ok(card.checks?.lines.some((line) => /신발장/.test(line.text)));
  assert.equal(group.lines[0].flagged, true);
  assert.equal(group.lines[1].flagged, true);
  assert.equal(group.lines[1].ms, null);
  assert.equal(group.lines[2].flagged, false);
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
