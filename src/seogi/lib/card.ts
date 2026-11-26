// The one-page summary ("한 장 요약"): what the card shows, taken from the
// minutes alone. Nothing is made up and nothing is guessed at: a part the
// minutes have nothing for is left out, a line the checks doubt is not set
// down as a fact but goes to "확인 필요", and every line keeps the moment of
// the recording it rests on.

import type { CheckReason, Entry, Meeting, TalkStats } from "@/bindings";
import { shareBar, speakerTones } from "./speakers";
import { sideOf, taskLine } from "./tasks";

/** The two shapes the card is drawn in: a phone picture (4:5, 1080×1350)
 *  and a printed page (A4). */
export type CardFormat = "kakao" | "a4";

/** One line of a part of the card. */
export interface CardLine {
  text: string;
  /** Where in the recording it was said; null when the minutes cite none. */
  ms: number | null;
  /** Who and by when, for a task; the reason, for a line to check. */
  note: string;
  /** The checks doubt it: its time is drawn in red. */
  doubted: boolean;
  /** A task the card cannot vouch for (a small doubt, or no moment of the
   *  recording behind it): marked "확인 필요" where it stands. */
  flagged?: boolean;
}

/** A part of the card that lists lines, with how many did not fit. */
export interface CardList {
  title: string;
  lines: CardLine[];
  /** Lines of the part left out for room. */
  more: number;
}

/** The numbers said: the budget drawn large when the minutes give one,
 *  then the other lines that carry an amount or a size. */
export interface CardNumbers {
  lead: { value: string; unit: string; line: CardLine } | null;
  lines: CardLine[];
  more: number;
}

/** The story of the meeting along its length, from the minutes' own story
 *  parts and the moments they cite. */
export interface CardFlow {
  parts: { title: string; startMs: number; endMs: number }[];
  totalMs: number;
  /** Marks above the band: decisions (ink) and lines to check (red). */
  ticks: { ms: number; red: boolean }[];
}

export interface CardPerson {
  name: string;
  /** The speaker's tone (1 … 4), or null for a name the minutes know only
   *  from the meeting's people. */
  tone: number | null;
}

export interface CardShare {
  name: string;
  tone: number;
  percent: number;
}

export interface Card {
  title: string;
  sub: string | null;
  /** The kind of minutes (the template's name). */
  kind: string | null;
  startedAtMs: number;
  durationMs: number;
  people: CardPerson[];
  /** Every line to be checked, shown or not: those the checks put to the
   *  person and the tasks marked "확인 필요" where they stand. */
  checkCount: number;
  checks: CardList | null;
  decisions: CardList | null;
  numbers: CardNumbers | null;
  /** At most two groups of tasks: ours, then theirs. */
  tasks: CardList[];
  flow: CardFlow | null;
  /** Who talked how much; empty when fewer than two talked. */
  share: CardShare[];
}

/** What the card needs from the screens' words. */
export interface CardWords {
  untitled: string;
  /** "도하린 고객" from a customer's name. */
  customer: (name: string) => string;
  /** A speaker's name as shown ("화자 2" → "말한 사람 2"). */
  speaker: (name: string) => string;
  /** The reason a line is to be checked, in words. */
  reason: (reason: CheckReason) => string;
}

/** How many lines each part may take at most, by shape. The A4 page is
 *  taller, so its parts take more. */
type Room = {
  decisions: number;
  numbers: number;
  tasks: number;
  checks: number;
};
const ROOM: Record<CardFormat, Room> = {
  kakao: { decisions: 4, numbers: 3, tasks: 3, checks: 3 },
  a4: { decisions: 7, numbers: 5, tasks: 6, checks: 5 },
};
/** Which part gives up a line first when two have as many. */
const GIVES: (keyof Room)[] = ["tasks", "decisions", "numbers", "checks"];

/**
 * The room after `fit` steps down, for when long lines do not fit (the
 * screen measures and steps down): each step takes a line from the part
 * that has the most, never below one.
 */
export function roomAt(format: CardFormat, fit: number): Room {
  const room = { ...ROOM[format] };
  for (let step = 0; step < fit; step++) {
    const part = GIVES.reduce((most, key) =>
      room[key] > room[most] ? key : most,
    );
    if (room[part] <= 1) break;
    room[part] -= 1;
  }
  return room;
}

/** How many steps down a shape has: `fit` runs from 0 to this less one. */
export function fitSteps(format: CardFormat): number {
  const room = ROOM[format];
  return GIVES.reduce((n, key) => n + room[key] - 1, 0) + 1;
}

/** The longest title the masthead takes before it is cut with "…" (two
 *  lines of the large type). */
export const TITLE_MAX = 34;
/** The longest line a part takes before it is cut with "…". */
export const LINE_MAX = 64;

/** `text` cut to `max` letters with "…", on a word's end when one is near. */
export function shorten(text: string, max: number): string {
  const letters = Array.from(text.trim());
  if (letters.length <= max) return letters.join("");
  const cut = letters.slice(0, max - 1).join("");
  const space = cut.lastIndexOf(" ");
  const kept = space >= max * 0.6 ? cut.slice(0, space) : cut;
  return `${kept.replace(/[\s,.·:;-]+$/, "")}…`;
}

/** A line as the card writes it: without the sentence's last full stop. */
const tidy = (text: string) => shorten(text.replace(/\.\s*$/, ""), LINE_MAX);

const firstMs = (entry: Entry) =>
  entry.sources.length > 0
    ? Math.min(...entry.sources.map((source) => source.ms))
    : null;

const doubted = (entry: Entry) => (entry.reasons ?? []).length > 0;
/** Doubted, or with the small doubt the minutes show faintly: not set down
 *  on the card as decided or as a number. */
const unsure = (entry: Entry) => doubted(entry) || Boolean(entry.faint);

/** One amount of money as said, whole: "4500만원", "1억 2천만 원",
 *  "3억 5000만원", "280만 원". Every number but the last ends in its unit,
 *  so the pattern reads a line only one way (no runaway backtracking). */
const MONEY =
  /(?=\d)(?:\d[\d,]*(?:\.\d+)?\s*(?:억|천만|만|천)\s*)*(?:\d[\d,]*(?:\.\d+)?\s*)?원/g;
/** How much of a line is read for its amount. */
export const MONEY_SPAN = 500;
/** A price per something ("평당 250만 원", "㎡당", "단가"): not the whole. */
const PER =
  /(?:평|㎡|m²|제곱미터|미터|m|개|장|대|명|시간|일|롤|박스)\s*당|단가/;
/** A range between two numbers: "4500~5000만원", "4천-5천만 원". */
const RANGE = /\d\s*(?:(?:억|천만|만|천)\s*)?(?:~|∼|〜|–|—|-|부터)\s*\d/;

/**
 * The one amount of money a line says, split for drawing large: "1억
 * 2천만 원" → "1억 2천" + "만 원". None when the line says no amount, more
 * than one ("5000만 원에서 4500만 원으로"), or a range: then which one
 * stands is not for the card to pick.
 */
export function moneyOf(said: string): { value: string; unit: string } | null {
  // A budget is said early in a line; a cap keeps the patterns short work.
  const text = said.slice(0, MONEY_SPAN);
  if (RANGE.test(text)) return null;
  const found = text.match(MONEY) ?? [];
  if (found.length !== 1) return null;
  const parts = [
    ...found[0].matchAll(/(\d[\d,]*(?:\.\d+)?)\s*(억|천만|만|천)?/g),
  ].map(([, digits, unit]) => ({ digits: grouped(digits), unit: unit ?? "" }));
  const last = parts[parts.length - 1];
  // "천만" is 천 of 만: the 천 stays with the number, the 만 goes to the unit.
  const tail =
    last.unit === "천만"
      ? { inValue: "천", unit: "만 원" }
      : last.unit === "천"
        ? { inValue: "천", unit: "원" }
        : { inValue: "", unit: last.unit ? `${last.unit} 원` : "원" };
  const value = [
    ...parts.slice(0, -1).map((part) => `${part.digits}${part.unit}`),
    `${last.digits}${tail.inValue}`,
  ].join(" ");
  return { value, unit: tail.unit };
}
/** Words that make an amount the whole of it: the budget, the total. */
const TOTAL = /예산|총액|총\s*공사비|전체\s*금액|합계/;
/** A number with what it counts: money, a size, an area, a share. */
const AMOUNT =
  /\d[\d,]*(?:\.\d+)?\s*(?:억|천만|만\s*원|만원|천\s*원|원|mm|밀리|cm|센티|미터|m²|㎡|평|%|퍼센트)/;

/** A heading that says its lines are not settled: "아직 안 정한 것",
 *  "결정 보류", "결정할 것", "합의 안 된 것", "결정 전", "미정". "안"
 *  counts only as its own word: "제안한 것", "안내한 것" are settled. */
const NOT_SETTLED =
  /보류|할\s*것|(?:^|\s)(?:안|못)\s*(?:된|한|정한|정해진)(?:\s|$)|않은|정하지|미정|아직|(?:결정|합의|확정)\s*전/;
type Heading = { key: string; title: string; kind?: string };
const isOpen = (section: Heading) =>
  section.key === "open" || NOT_SETTLED.test(section.title);
const isDecisions = (section: Heading) =>
  !isOpen(section) &&
  (section.key === "decisions" ||
    /결정|정한|합의|약속한|확정/.test(section.title));
const isBudget = (section: Heading) =>
  section.key === "budget" || /예산|금액|비용/.test(section.title);
/** Where the numbers of what was settled are: the budget, the materials
 *  and sizes, the site, the schedule. Not worries, wishes, risks or talk,
 *  whose amounts are no one's figure. */
const isFigures = (section: Heading) =>
  section.kind === "items" &&
  !isOpen(section) &&
  !isDecisions(section) &&
  !/우려|걱정|요청|바람|희망|리스크|위험|논의|용건|이야기/.test(
    section.title,
  ) &&
  (["budget", "materials", "site", "schedule"].includes(section.key) ||
    /예산|금액|비용|견적|자재|사양|치수|규격|현장|일정/.test(section.title));

/** "4500" → "4,500": the digits as said, grouped. */
function grouped(digits: string): string {
  const [whole, part] = digits.replace(/,/g, "").split(".");
  const withCommas = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return part ? `${withCommas}.${part}` : withCommas;
}

/** A speaker's id → tone, the same as the minutes screen gives. */
function tonesOf(meeting: Meeting) {
  const transcript = meeting.transcript;
  const order = (transcript?.segments ?? []).map((line) => line.speaker);
  return speakerTones(
    [...(transcript?.speakers ?? []).map((speaker) => speaker.id), ...order],
    order,
  );
}

/** The people of a meeting, whose names a task line may leave out. */
const peopleOf = (meeting: Meeting) => [
  ...(meeting.people ?? []),
  ...(meeting.transcript?.speakers ?? []).map((speaker) => speaker.name),
];

function list<T>(
  title: string,
  items: T[],
  room: number,
  line: (item: T) => CardLine,
): CardList | null {
  if (items.length === 0) return null;
  return {
    title,
    lines: items.slice(0, room).map(line),
    more: Math.max(0, items.length - room),
  };
}

/** What the card shows for `meeting`, from its minutes and `talk`; `fit`
 *  steps down how many lines each part takes (0 is the most). */
export function cardOf(
  meeting: Meeting,
  talk: TalkStats | null,
  format: CardFormat,
  words: CardWords,
  fit = 0,
): Card {
  const room = roomAt(format, Math.max(0, fit));
  const minutes = meeting.minutes?.[0];
  const sections = minutes?.sections ?? [];

  // The masthead: the customer when the meeting has one, else its title.
  const customer = (meeting.customer ?? "").trim();
  const ownTitle = meeting.title.trim();
  const headline = customer
    ? words.customer(customer)
    : ownTitle || words.untitled;
  const subs = (customer ? [minutes?.title, ownTitle] : [minutes?.title])
    .map((text) => (text ?? "").trim())
    .filter((text) => text && text !== headline && text !== ownTitle);
  const sub =
    subs[0] ?? (customer && ownTitle && ownTitle !== headline ? ownTitle : "");

  // People: the speakers in their tones, else the meeting's names.
  const tones = tonesOf(meeting);
  const speakers = meeting.transcript?.speakers ?? [];
  const spoke = new Set(
    (meeting.transcript?.segments ?? []).map((line) => line.speaker),
  );
  const people: CardPerson[] =
    speakers.length > 0
      ? speakers
          .filter((speaker) => spoke.has(speaker.id))
          .map((speaker) => ({
            name: words.speaker(speaker.name),
            tone: tones.get(speaker.id) ?? null,
          }))
      : (meeting.people ?? [])
          .filter((name) => name.trim())
          .map((name) => ({ name, tone: null }));

  // What the checks put to the person, in the order of the minutes.
  const toCheck = sections.flatMap((section) =>
    section.entries.filter((entry) => doubted(entry) && !entry.faint),
  );
  const checks = list(
    "",
    toCheck.sort((a, b) => (firstMs(a) ?? 0) - (firstMs(b) ?? 0)),
    room.checks,
    (entry) => ({
      text: tidy(entry.text),
      ms: firstMs(entry),
      note: words.reason(entry.reasons[0]),
      doubted: true,
    }),
  );

  // Decisions: only the ones the checks did not doubt, not even faintly.
  const decisionSection = sections.find(
    (section) => section.kind === "items" && isDecisions(section),
  );
  const decided = (decisionSection?.entries ?? []).filter(
    (entry) => !unsure(entry),
  );
  const decisions = decisionSection
    ? list(decisionSection.title, decided, room.decisions, (entry) => ({
        text: tidy(entry.text),
        ms: firstMs(entry),
        note: "",
        doubted: false,
      }))
    : null;

  // Numbers: lines that carry an amount, from the parts that hold figures
  // (not the decisions, shown already, nor what was left open, worried
  // about or wished for).
  const numberSections = sections.filter(isFigures);
  const budget = numberSections.find(isBudget);
  // Drawn large only when the line says it is the budget or the total, and
  // says one amount, not two, a range or a price per something.
  const leadEntry = budget?.entries.find(
    (entry) =>
      !unsure(entry) &&
      TOTAL.test(entry.text) &&
      !PER.test(entry.text) &&
      moneyOf(entry.text) !== null,
  );
  const leadMoney = leadEntry ? moneyOf(leadEntry.text) : null;
  let lead: CardNumbers["lead"] = null;
  if (leadEntry && leadMoney) {
    lead = {
      ...leadMoney,
      line: {
        text: tidy(leadEntry.text),
        ms: firstMs(leadEntry),
        note: "",
        doubted: false,
      },
    };
  }
  const amounts = numberSections
    .flatMap((section) => section.entries)
    .filter(
      (entry) =>
        entry !== leadEntry && !unsure(entry) && AMOUNT.test(entry.text),
    )
    .filter(
      // The same words under two headings are one line.
      (entry, index, all) =>
        all.findIndex((other) => other.text === entry.text) === index,
    )
    .sort((a, b) => (firstMs(a) ?? 0) - (firstMs(b) ?? 0));
  const numberRoom = lead ? room.numbers - 1 : room.numbers;
  const numbers: CardNumbers | null =
    lead || amounts.length > 0
      ? {
          lead,
          lines: amounts.slice(0, numberRoom).map((entry) => ({
            text: tidy(entry.text),
            ms: firstMs(entry),
            note: "",
            doubted: false,
          })),
          more: Math.max(0, amounts.length - numberRoom),
        }
      : null;

  // Tasks: ours first, then the other side's; at most two groups.
  const names = peopleOf(meeting);
  const order = { us: 0, customer: 1, them: 1 } as const;
  const groups = sections
    .filter((section) => section.kind === "actions")
    .filter((section) => section.entries.length > 0)
    .map((section, index) => ({ section, index, side: sideOf(section) }))
    .sort(
      (a, b) =>
        (a.side ? order[a.side] : 2) - (b.side ? order[b.side] : 2) ||
        a.index - b.index,
    )
    .slice(0, 2);
  // A task the checks doubt is listed under 확인 필요, like a decision; one
  // with a faint doubt or no moment behind it stays, marked.
  const tasks = groups
    .map(({ section }) =>
      list(
        section.title,
        section.entries.filter((entry) => !doubted(entry)),
        room.tasks,
        (entry) => {
          const { who, text } = taskLine(section, entry, names);
          const flagged = Boolean(entry.faint) || firstMs(entry) === null;
          return {
            text: tidy(text),
            ms: firstMs(entry),
            note: [who, entry.due.trim()].filter(Boolean).join(" · "),
            doubted: flagged,
            flagged,
          };
        },
      ),
    )
    .filter((group): group is CardList => group !== null);
  // Tasks marked where they stand count among what is to be checked, so
  // the number at the top agrees with every red mark below.
  const flaggedTasks = groups
    .flatMap(({ section }) => section.entries)
    .filter(
      (entry) =>
        !doubted(entry) && (Boolean(entry.faint) || firstMs(entry) === null),
    ).length;

  // The flow: the story parts that have a title and cite a moment, from the
  // first moment each cites to the next part's. Fewer than two say nothing
  // about how the meeting went, so then there is no flow.
  const total = Math.max(1, meeting.duration_ms);
  const starts = (minutes?.story ?? [])
    .filter((part) => part.title.trim() && part.sources.length > 0)
    .map((part) => ({
      title: part.title.trim(),
      startMs: Math.min(...part.sources.map((source) => source.ms)),
    }))
    .filter((part) => part.startMs < total)
    .sort((a, b) => a.startMs - b.startMs)
    .filter(
      (part, index, all) =>
        index === 0 || part.startMs > all[index - 1].startMs,
    );
  const flow: CardFlow | null =
    starts.length >= 2
      ? {
          parts: starts.map((part, index) => ({
            ...part,
            endMs: starts[index + 1]?.startMs ?? total,
          })),
          totalMs: total,
          ticks: [
            ...decided.map((entry) => ({ ms: firstMs(entry), red: false })),
            ...toCheck.map((entry) => ({ ms: firstMs(entry), red: true })),
          ]
            .filter(
              (tick): tick is { ms: number; red: boolean } =>
                tick.ms !== null && tick.ms <= total,
            )
            .sort((a, b) => a.ms - b.ms),
        }
      : null;

  // Who talked how much, in the speakers' tones.
  const pieces = shareBar(talk?.speakers ?? []);
  const share =
    pieces.length >= 2
      ? pieces.map((piece) => ({
          name: words.speaker(
            speakers.find((speaker) => speaker.id === piece.speaker)?.name ??
              `화자 ${piece.speaker}`,
          ),
          tone: tones.get(piece.speaker) ?? 1,
          percent: piece.percent,
        }))
      : [];

  return {
    title: shorten(headline, TITLE_MAX),
    sub: sub ? shorten(sub, LINE_MAX) : null,
    kind: minutes?.template_name?.trim() || null,
    startedAtMs: meeting.started_at_ms,
    durationMs: meeting.duration_ms,
    people,
    checkCount: toCheck.length + flaggedTasks,
    checks,
    decisions,
    numbers,
    tasks,
    flow,
    share,
  };
}
