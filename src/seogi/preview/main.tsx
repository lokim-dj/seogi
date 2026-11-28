// Seogi's screens in a plain browser, on made-up data, without the backend.
// For looking at a screen while working on it and for taking pictures of
// every screen in one go (scripts/seogi-screens.mjs). Not part of the app.
//
//   http://localhost:1420/src/seogi/preview/index.html?screen=home
//   screen = home | home-going | home-going-long | home-many | minutes |
//            recording | onboarding | settings | waiting | banner-starting |
//            banner-ready | settings-dictation |
//            indicator-listening | indicator-tidying | indicator-note |
//            indicator-nowhere | card | card-empty | minutes-edited |
//            minutes-editing | minutes-fixed | settings-vocabulary
//   &claude=missing|signed_out|signed_in&codex=… sets where the two
//   subscription tools stand (settings, onboarding).
//   &summary=key starts on '내 API 키' (Anthropic); &key=anthropic has its
//   key saved.
//   &os=windows shows the screens as on Windows (no Mac Calendar, no
//   macOS permission rows).
//   &step=2 or &step=3 opens the first-run screen on that step.
//   minutes-edited has a task changed, one taken out, one added and a
//   transcript line fixed; minutes-editing opens the boxes for changing a
//   task and a line. minutes-fixed has a misheard word fixed by the user
//   and one fixed by the vocabulary as the recording was transcribed;
//   settings-vocabulary opens 설정 › 녹음 › 용어집.
//   card opens the one-page summary of the made-up consultation; card-empty
//   the same meeting with no decisions, no numbers, nothing to check and a
//   long title. What the card's save keys would write is kept in
//   window.__savedCard (for scripts/seogi-screens.mjs).

import React from "react";
import ReactDOM from "react-dom/client";
import { emit } from "@tauri-apps/api/event";
import {
  mockConvertFileSrc,
  mockIPC,
  mockWindows,
} from "@tauri-apps/api/mocks";
import type {
  Config,
  Dictation,
  DictationStatus,
  DriveStatus,
  Found,
  FoundPlace,
  Meeting,
  MeetingSummary,
  Misheard,
  Naming,
  PhoneStatus,
  Readiness,
  RecordingState,
  Segment,
  SignIn,
  SignIns,
  Template,
  Tool,
  Upcoming,
  Word,
  WordFix,
} from "@/bindings";
import call from "../../../src-tauri/src/seogi/templates/call.json";
import consult from "../../../src-tauri/src/seogi/templates/consult.json";
import meetingTemplate from "../../../src-tauri/src/seogi/templates/meeting.json";
import memo from "../../../src-tauri/src/seogi/templates/memo.json";
import vendor from "../../../src-tauri/src/seogi/templates/vendor.json";
import consultMeeting from "./fixtures/consult.json";

const screen =
  new URLSearchParams(window.location.search).get("screen") ?? "home";
const now = Date.now();
const minute = 60000;
const today = (hours: number, minutes: number) => {
  const d = new Date(now);
  d.setHours(hours, minutes, 0, 0);
  return d.getTime();
};

const done = consultMeeting as unknown as Meeting;
done.started_at_ms = today(10, 30);
if (new URLSearchParams(window.location.search).get("mic") === "lost") {
  // `?screen=minutes&mic=lost`: the lid closed for a moment mid-meeting.
  done.gaps = [{ at_ms: 100_000, gap_ms: 17_000 }];
}
if (screen === "minutes-edited" || screen === "minutes-editing") {
  // What the user changed: a task's words, one taken out, one added, and a
  // transcript line fixed (src-tauri/src/seogi/minutes.rs, transcript.rs).
  const tasks = done.minutes?.[0]?.sections.find(
    (section) => section.kind === "actions",
  );
  if (tasks && screen === "minutes-edited") {
    const changed = tasks.entries[2];
    changed.original = changed.text;
    changed.text = "공사 신고와 엘리베이터 보양을 착공 전에 처리하기로 했다.";
    const [gone] = tasks.entries.splice(1, 1);
    tasks.removed = [{ index: 1, entry: gone }];
    tasks.entries.push({
      text: "현관 중문 견적을 두 곳에서 받아 보기로 했다.",
      owner: "",
      due: "",
      sources: [],
      reasons: [],
      done: false,
      faint: false,
      original: null,
      added: true,
      id: "added-1",
    });
    const line = done.transcript?.segments[1];
    if (line) line.edited = line.text.replace(/\s*$/, " 거실부터 볼게요.");
  }
}
if (screen === "minutes-fixed") {
  // A word the user fixed, and one the vocabulary fixed as the recording was
  // transcribed (src-tauri/src/seogi/transcript.rs, WordFix).
  const segments = done.transcript?.segments ?? [];
  const user = segments[5];
  if (user) {
    const at = Array.from(user.text).join("").indexOf("우물천장탄");
    user.fixes = [{ at, from: "우물천장탄", to: "우물천장", auto: false }];
    user.edited = user.text.replace("우물천장탄", "우물천장");
  }
  // The vocabulary's fix is in the words as transcribed already.
  const auto = segments[9];
  if (auto) auto.fixes = [{ at: 4, from: "삼연동", to: "3연동", auto: true }];
}
if (screen === "card-empty") {
  // Nothing decided, no amounts, nothing doubted, and a long title.
  done.customer = "";
  done.title =
    "한빛아파트 103동 1502호 거실 천장과 바닥, 주방 상부장과 붙박이장까지 길게 이야기한 두 번째 상담";
  const minutes = done.minutes?.[0];
  if (minutes) {
    minutes.title = done.title;
    minutes.sections = minutes.sections.filter(
      (section) =>
        section.kind === "actions" ||
        section.key === "open" ||
        section.key === "concerns",
    );
    for (const section of minutes.sections) {
      for (const entry of section.entries) entry.reasons = [];
    }
  }
}

// Whether the preview may read the Mac's calendars: asked once, like the
// system does.
let macAccess = "not_determined";
// The first-run screen starts with the microphone not yet allowed, so its
// permission rows show their buttons.
let micAllowed = screen !== "onboarding";

const config: Config = {
  library_dir: "",
  watched: [
    {
      path: "/Users/me/Library/CloudStorage/GoogleDrive-me@work.example/내 드라이브/녹음함",
      enabled: true,
      template_id: "",
      since_ms: 0,
    },
  ],
  language: "ko",
  diarize: true,
  speech_model: "",
  summary:
    new URLSearchParams(window.location.search).get("summary") === "key"
      ? { kind: "anthropic", model: "claude-opus-5-5", key: "anthropic" }
      : { kind: "claude_cli", model: "claude-sonnet-5-5" },
  vocabulary_path: "",
  input_device: "",
  calendar_urls: ["https://calendar.example/secret.ics"],
  mac_calendars: [],
  remind: true,
  remind_minutes_before: 1,
  set_up: screen !== "onboarding",
  record_shortcut: "Ctrl+Alt+R",
  mark_shortcut: "Ctrl+Alt+M",
  muted_events: ["weekly"],
  minutes_beside_source: true,
  drive_upload_own: true,
  drive_upload_audio: true,
  dictation: {
    enabled: screen === "settings-dictation" || screen === "onboarding",
    tidy: screen === "settings-dictation",
    tidy_with: {
      kind: "ollama",
      base_url: "http://localhost:11434",
      model: "gemma3:4b",
    },
    numbers: true,
  },
};

const qwen = "handy-computer/Qwen3-ASR-1.7B-gguf/Qwen3-ASR-1.7B-Q8_0.gguf";
const dictation: Dictation = {
  enabled: config.dictation.enabled,
  shortcut: "option+space",
  press: "both",
  typing: "paste",
  model: qwen,
  sounds: false,
  fillers: true,
  tidy: config.dictation.tidy,
  tidy_with: config.dictation.tidy_with,
  numbers: config.dictation.numbers,
  keep: 5,
};

// The recent dictations; "지우기" takes them out for the rest of the visit.
let recentDictations = [
  {
    id: 2,
    text: "내일 회의는 3시로 옮기고요, 자료는 다음 두 가지를 준비해 주세요.\n1. 매출표\n2. 일정표",
    at_ms: now - 4 * minute,
  },
  { id: 1, text: "네, 오늘 안에 보내 드릴게요.", at_ms: now - 31 * minute },
];
const dictationStatus = (): DictationStatus => ({
  permitted: screen !== "settings-dictation" && screen !== "onboarding",
  active: dictation.enabled,
  handy_installed: true,
  handy_running: true,
  shortcut_trouble: null,
  taken: [
    { keys: "Ctrl+Alt+R", by: "record" },
    { keys: "Ctrl+Alt+M", by: "mark" },
    { keys: "option+space", by: "handy" },
    { keys: "option+shift+space", by: "handy" },
  ],
  models: [
    {
      id: qwen,
      label: "Qwen3 ASR 1.7B",
      downloaded: true,
      size_mb: 2100,
      speed: "fair",
      accurate: true,
      recommended: false,
    },
    {
      id: "handy-computer/Fun-ASR-MLT-Nano-2512-gguf/Fun-ASR-MLT-Nano-2512-Q8_0.gguf",
      label: "Fun-ASR Nano",
      downloaded: false,
      size_mb: 1000,
      speed: "fast",
      accurate: true,
      recommended: true,
    },
  ],
  tidy_choices: [
    {
      provider: {
        kind: "ollama",
        base_url: "http://localhost:11434",
        model: "gemma3:4b",
      },
      label: "gemma3:4b",
      speed: "fast",
      last_ms: 620,
      metered: false,
    },
    {
      provider: {
        kind: "ollama",
        base_url: "http://localhost:11434",
        model: "gemma3:12b",
      },
      label: "gemma3:12b",
      speed: "fair",
      last_ms: null,
      metered: false,
    },
    {
      provider: { kind: "claude_cli", model: "haiku" },
      label: "Claude Haiku",
      speed: "slow",
      last_ms: null,
      metered: true,
    },
  ],
  ollama_running: true,
  recent: [...recentDictations],
});

// `?speech=missing` shows the speech model not downloaded yet, with the
// old default standing in.
const speechMissing =
  new URLSearchParams(window.location.search).get("speech") === "missing";
const readiness: Readiness = {
  speech_model: qwen,
  speech_model_ready: !speechMissing,
  speech_model_stand_in: speechMissing
    ? "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf"
    : null,
  speech_model_bytes: 2083 * 1024 * 1024,
  speech_model_chosen: false,
  speech_models: [
    {
      id: qwen,
      downloaded: !speechMissing,
      downloading: false,
      bytes: 2083 * 1024 * 1024,
    },
    {
      id: "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf",
      downloaded: true,
      downloading: false,
      bytes: 845 * 1024 * 1024,
    },
    {
      id: "handy-computer/Fun-ASR-MLT-Nano-2512-gguf/Fun-ASR-MLT-Nano-2512-Q8_0.gguf",
      downloaded: true,
      downloading: false,
      bytes: 849 * 1024 * 1024,
    },
  ],
  diarizer_ready: true,
  ffmpeg: true,
  ollama_running: true,
  ollama_models: ["gemma3:12b"],
  claude_cli: true,
  codex_cli: false,
  summary: "claude sonnet",
  summary_ready: true,
  summary_model_refused: false,
  summary_fallback: null,
  claude_models: [
    { id: "claude-opus-5-5", label: "Opus 5.5" },
    { id: "claude-sonnet-5-5", label: "Sonnet 5.5" },
  ],
  codex_models: [
    { id: "gpt-6.2-astra", label: "GPT-6.2 Astra" },
    { id: "gpt-6.2-sol", label: "GPT-6.2 Sol" },
  ],
  vocabulary_path: "/Users/me/Documents/서기 용어집",
  vocabulary_terms: 312,
  library: "/Users/me/Documents/서기",
  drives: [
    {
      account: "me@example.com",
      inbox:
        "/Users/me/Library/CloudStorage/GoogleDrive-me@example.com/내 드라이브/녹음함",
      inbox_exists: false,
    },
    {
      account: "me@work.example",
      inbox:
        "/Users/me/Library/CloudStorage/GoogleDrive-me@work.example/내 드라이브/녹음함",
      inbox_exists: true,
    },
  ],
  local_inbox: "/Users/me/녹음함",
  microphone: "granted",
};

const summary = (over: Partial<MeetingSummary>): MeetingSummary => ({
  id: "x",
  title: "",
  started_at_ms: now,
  duration_ms: 0,
  origin: "recorded",
  status: "done",
  stage: "",
  percent: 0,
  error: "",
  kind: "",
  template_id: "",
  summary: "",
  actions: 0,
  verified: 0,
  to_check: 0,
  done: 0,
  speakers: 0,
  folder: "/Users/me/Documents/서기/2026-11-24 1030 이서진 1차 상담",
  ...over,
});

const minutes = done.minutes?.[0];
const meetings: MeetingSummary[] = [
  summary({
    id: done.id,
    title: "[회의실A] 이서진 / 마감재 선택",
    started_at_ms: today(10, 30),
    duration_ms: 52 * minute,
    kind: "고객 상담",
    template_id: "consult",
    summary: minutes?.summary ?? "",
    actions: 8,
    verified: minutes?.verified ?? 0,
    to_check: 1,
  }),
  summary({
    id: "working",
    title: "[미팅] 새봄마루 / 원목마루 단가",
    started_at_ms: today(9, 0),
    duration_ms: 38 * minute,
    origin: "watched",
    status: "working",
    stage: "writing_minutes",
    percent: 82,
    template_id: "vendor",
  }),
  summary({
    id: "yesterday-1",
    title: "[회의] 11월 착공 일정 조율",
    started_at_ms: today(17, 10) - 24 * 60 * minute,
    duration_ms: 44 * minute,
    kind: "회의",
    template_id: "meeting",
    actions: 4,
    verified: 21,
  }),
  summary({
    id: "yesterday-2",
    title: "[회의실B] 정하람 / 2차 상담",
    started_at_ms: today(13, 0) - 24 * 60 * minute,
    duration_ms: 81 * minute,
    origin: "watched",
    kind: "고객 상담",
    template_id: "consult",
    actions: 3,
    verified: 17,
    to_check: 2,
  }),
  summary({
    id: "transcribing",
    title: "[미팅] 새봄창호 / 중문 실측",
    started_at_ms: today(15, 30) - 4 * 24 * 60 * minute,
    duration_ms: 27 * minute,
    origin: "watched",
    status: "working",
    stage: "transcribing",
    percent: 62,
    template_id: "vendor",
  }),
  summary({
    id: "queued",
    title: "",
    started_at_ms: today(11, 0) - 4 * 24 * 60 * minute,
    duration_ms: 12 * minute,
    origin: "imported",
    status: "queued",
  }),
  summary({
    id: "older",
    title: "[회의] 주간 공정 회의",
    started_at_ms: today(9, 30) - 4 * 24 * 60 * minute,
    duration_ms: 35 * minute,
    kind: "회의",
    template_id: "meeting",
    actions: 5,
    verified: 19,
  }),
];

const upcoming: Upcoming[] = [
  {
    event: {
      uid: "consult",
      title: "[회의실A] 김서윤 / 초도 상담",
      start_ms: now + 12 * minute,
      end_ms: now + 72 * minute,
      location: "회의실A",
      attendees: ["박도현"],
    },
    muted: false,
  },
  {
    event: {
      uid: "vendor",
      title: "[미팅] 새봄타일 / 욕실 타일 샘플",
      start_ms: now + 162 * minute,
      end_ms: now + 222 * minute,
      location: "",
      attendees: [],
    },
    muted: false,
  },
  {
    event: {
      uid: "weekly",
      title: "[회의] 주간 공정 회의",
      start_ms: now + 252 * minute,
      end_ms: now + 312 * minute,
      location: "",
      attendees: [],
    },
    muted: true,
  },
];

// An event going on since early afternoon, and the next one soon: the
// record key names the going one, the next one is beside it
// (screen=home-going; home-going-long gives the going one a long name).
const going: Upcoming[] = [
  {
    event: {
      uid: "review",
      title: "[회의실B] 한결 / 3차 디자인 검토",
      start_ms: now - 163 * minute,
      end_ms: now + 77 * minute,
      location: "회의실B",
      attendees: [],
    },
    muted: false,
  },
  {
    event: {
      uid: "site",
      title: "[미팅] 새봄타일 / 현장 실측",
      start_ms: now + 17 * minute,
      end_ms: now + 77 * minute,
      location: "",
      attendees: [],
    },
    muted: false,
  },
];

const goingLong: Upcoming[] = going.map((upcoming, index) =>
  index === 0
    ? {
        ...upcoming,
        event: {
          ...upcoming.event,
          title:
            "[회의실B] 한결 / 3차 디자인 검토 · 주방 상판 · 욕실 타일 · 조명 위치 최종 확인",
        },
      }
    : upcoming,
);

// A full afternoon: one leads, three are listed, the rest counted
// (screen=home-many).
const many: Upcoming[] = [
  ["m1", "[회의실A] 윤슬 / 2차 상담", 25],
  ["m2", "[미팅] 새봄조명 / 조명 견본", 85],
  ["m3", "[회의] 공정 점검", 130],
  ["m4", "[회의실B] 다온 / 계약 전 확인", 190],
  ["m5", "[미팅] 새봄창호 / 견적 설명", 250],
].map(([uid, title, from]) => ({
  event: {
    uid: String(uid),
    title: String(title),
    start_ms: now + Number(from) * minute,
    end_ms: now + (Number(from) + 50) * minute,
    location: "",
    attendees: [],
  },
  muted: false,
}));

const recording: RecordingState = {
  id: "recording",
  started_at_ms: now - 23 * minute - 41000,
  elapsed_ms: 23 * minute + 41000,
  level: 0.31,
  marks_ms: [4 * minute + 12000, 12 * minute + 41000, 19 * minute + 5000],
  device: "MacBook Pro 마이크",
  no_sound: false,
  // `?screen=recording&mic=lost`: the lid closed mid-meeting.
  reconnecting:
    new URLSearchParams(window.location.search).get("mic") === "lost",
};

const recordingMeeting: Meeting = {
  ...done,
  id: "recording",
  title: "김서윤 초도 상담",
  status: "recording",
  transcript: null,
  minutes: [],
  template_id: "consult",
  customer: "김서윤",
  people: ["박도현"],
  marks_ms: recording.marks_ms,
  mark_notes: ["예산 이야기", "", "착공일"],
  event: {
    uid: "consult",
    title: "[회의실A] 김서윤 / 초도 상담",
    start_ms: now - 24 * minute,
    end_ms: now + 36 * minute,
  },
};

/** A section of the made-up minutes, by its key. */
const taskSection = (key: unknown) =>
  done.minutes?.[0]?.sections.find((section) => section.key === key);

/** What the backend's search gives for the one meeting with minutes here,
 *  in the same order (src-tauri/src/seogi/store.rs, `Meeting::found`). */
function search(query: string): Found[] {
  const words = query.trim().toLowerCase();
  if (!words) return [];
  const has = (text: string) => text.toLowerCase().includes(words);
  const places: FoundPlace[] = [];
  const add = (
    kind: FoundPlace["kind"],
    text: string,
    section = "",
    index = 0,
    ms: number | null = null,
  ) => places.push({ kind, text, section, index, ms });
  if (has(done.title)) add("title", done.title);
  const minutes = done.minutes?.[0];
  if (minutes) {
    const entries = (sections: typeof minutes.sections) => {
      for (const section of sections) {
        section.entries.forEach((entry, index) => {
          if (has(entry.text)) add("entry", entry.text, section.key, index);
          else if (has(entry.owner)) {
            add("entry", `${entry.owner} ${entry.text}`, section.key, index);
          }
        });
      }
    };
    entries(minutes.sections.filter((s) => s.kind === "actions"));
    (minutes.story ?? []).forEach((part, index) => {
      if (has(part.text)) add("story", part.text, "", index);
      else if (has(part.title)) {
        add("story", `${part.title} · ${part.text}`, "", index);
      }
    });
    if ((minutes.story ?? []).length === 0 && has(minutes.summary)) {
      add("summary", minutes.summary);
    }
    const rest = minutes.sections.filter((s) => s.kind !== "actions");
    entries([
      ...rest.filter((s) => s.key !== "open"),
      ...rest.filter((s) => s.key === "open"),
    ]);
  }
  (done.transcript?.segments ?? []).forEach((segment, index) => {
    if (has(segment.text)) {
      add("transcript", segment.text, "", index, segment.start_ms);
    }
  });
  const people = (done.people ?? []).join(", ");
  if (has(people)) add("people", people);
  if (places.length === 0) return [];
  return [{ id: done.id, places: places.slice(0, 5), total: places.length }];
}

/** Each speaker's talking in a meeting, the way talk.rs measures it. */
function talkOf(meeting: Meeting) {
  const totals = new Map<number, number>();
  for (const line of meeting.transcript?.segments ?? []) {
    if (line.speaker === 0) continue;
    const ms = line.end_ms - line.start_ms;
    totals.set(line.speaker, (totals.get(line.speaker) ?? 0) + ms);
  }
  const all = [...totals.values()].reduce((n, ms) => n + ms, 0) || 1;
  return [...totals.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([speaker, talk_ms]) => ({
      speaker,
      talk_ms,
      share: Math.round((talk_ms * 1000) / all) / 10,
      turns: 5,
      longest_ms: 12000,
      longest_at_ms: 15000,
      questions: 1,
      cut_ins: 1,
    }));
}

const templates = [
  meetingTemplate,
  consult,
  vendor,
  call,
  memo,
] as unknown as Template[];

const windows =
  new URLSearchParams(window.location.search).get("os") === "windows";

// The vocabulary, as the settings list it: generic words, two learnt in a
// meeting (src-tauri/src/seogi/vocab.rs).
const todayIso = (() => {
  const d = new Date(now);
  const two = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())}`;
})();
let vocabularyWords: Word[] = [
  {
    term: "우물천장",
    misheard: [
      { form: "우물천장탄", added_on: todayIso, meeting: done.title },
      { form: "우물 천정", added_on: "", meeting: "" },
    ],
    editable: true,
  },
  { term: "간접등", misheard: [], editable: true },
  { term: "포세린 타일", misheard: [], editable: true },
  {
    term: "3연동",
    misheard: [{ form: "삼연동", added_on: "", meeting: "" }],
    editable: true,
  },
  {
    term: "방마루",
    misheard: [{ form: "방마로", added_on: "", meeting: "" }],
    editable: true,
  },
  ...[
    "걸레받이",
    "몰딩",
    "중문",
    "도배",
    "줄눈",
    "실리콘",
    "붙박이장",
    "아트월",
  ].map((term) => ({ term, misheard: [], editable: true })),
];

/** How many characters of a line as transcribed a fix covers. */
const covers = (fix: WordFix) =>
  Array.from(fix.auto ? fix.to : fix.from).length;

/** A line's words with the user's word fixes put in, as the backend reads
 *  it (the vocabulary's are in its words already). */
function refix(line: Segment) {
  const fixes = [...(line.fixes ?? [])].sort((a, b) => a.at - b.at);
  line.fixes = fixes;
  const text = Array.from(line.text);
  let out = "";
  let next = 0;
  for (const fix of fixes) {
    out += text.slice(next, fix.at).join("") + fix.to;
    next = fix.at + covers(fix);
  }
  out += text.slice(next).join("");
  line.edited = out === line.text ? null : out;
}

/** The places a misheard word is fixed in the preview's meeting: where it
 *  was picked, and where else it starts a word (simpler than the backend's
 *  rules, enough to walk the flow). */
function fixPlaces(atMs: number, at: number, from: string) {
  const segments = done.transcript?.segments ?? [];
  const n = Array.from(from).length;
  const places: { line: Segment; at: number }[] = [];
  for (const line of segments) {
    const rewritten =
      line.edited !== null &&
      line.edited !== undefined &&
      !(line.fixes ?? []).length;
    if (rewritten) continue;
    const text = Array.from(line.text);
    // The picked place, from the line as read to the line as transcribed.
    let picked = -1;
    if (line.start_ms === atMs) {
      picked = at;
      for (const fix of line.fixes ?? []) {
        if (fix.at < picked) picked += covers(fix) - Array.from(fix.to).length;
      }
    }
    for (let i = 0; i + n <= text.length; i += 1) {
      if (text.slice(i, i + n).join("") !== from) continue;
      const taken = (line.fixes ?? []).some(
        (fix) => i < fix.at + covers(fix) && fix.at < i + n,
      );
      const starts = i === 0 || !/[\p{L}\p{N}]/u.test(text[i - 1]);
      if (!taken && (i === picked || (starts && n >= 2)))
        places.push({ line, at: i });
    }
  }
  return places;
}

mockWindows("main");
mockConvertFileSrc(windows ? "windows" : "macos");
Object.assign(window, {
  __TAURI_OS_PLUGIN_INTERNALS__: windows
    ? {
        platform: "windows",
        os_type: "windows",
        family: "windows",
        arch: "x86_64",
        eol: "\r\n",
        version: "10.0.26100",
        exe_extension: "exe",
      }
    : {
        platform: "macos",
        os_type: "macos",
        family: "unix",
        arch: "aarch64",
        eol: "\n",
        version: "15.0.0",
        exe_extension: "",
      },
});

// Where the two subscription tools stand: `&claude=missing&codex=signed_in`
// picks a state for each. A sign-in or an installation finishes a few
// seconds after it is started, as it would in the browser.
const query = new URLSearchParams(window.location.search);
const signInState = (name: string, fallback: SignIn): SignIn => {
  const value = query.get(name);
  return value === "missing" || value === "signed_out" || value === "signed_in"
    ? value
    : fallback;
};
// The keys "saved" in the preview's keychain; `&key=anthropic` starts with
// one.
const savedKeys = new Set(query.getAll("key"));
const signIns: SignIns = {
  claude: signInState("claude", "signed_in"),
  codex: signInState("codex", "signed_out"),
};

// With '내 API 키' chosen and no key saved, the backend names the way that
// writes instead: the signed-in Claude, ChatGPT, or none.
const readinessNow = (): Readiness => {
  const keyMissing =
    new URLSearchParams(window.location.search).get("summary") === "key" &&
    !savedKeys.has("anthropic");
  if (!keyMissing) return readiness;
  return {
    ...readiness,
    summary_ready: false,
    summary_fallback:
      signIns.claude === "signed_in"
        ? { kind: "claude_cli", model: "" }
        : signIns.codex === "signed_in"
          ? { kind: "codex_cli", model: "" }
          : null,
  };
};

// What the screens asked of the backend, for scripts/seogi-flows.mjs to read.
const calls: { command: string; args: Record<string, unknown> }[] = [];
Object.assign(window, { __seogiCalls: calls });
let isRecording = screen === "recording";
const phone: PhoneStatus = {
  available: true,
  account: "someone@example.com",
  connected: true,
  needs_reconnect: false,
  connecting: false,
  pulling: false,
  last_look_ms: Date.now() - 4 * 60_000,
  brought_in: 2,
  trouble: "",
};
// Meetings in Drive: one put up already, and "지난 회의 올리기" offering
// the ones done before connecting.
const drive: DriveStatus = {
  uploading: false,
  waiting: 0,
  audio_waiting: 0,
  excluded: [],
  uploaded: [],
  trouble: "",
};
let backlogLeft = 7;
let atLogin = true;
const trashedRows: MeetingSummary[] = [];

mockIPC(
  (command, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    if (command.startsWith("seogi_") || command.startsWith("plugin:macos")) {
      calls.push({ command, args });
    }
    switch (command) {
      // The few commands that change something, so a flow can be walked.
      case "seogi_set_config":
        Object.assign(config, args.config as Config);
        return null;
      case "seogi_copy_last_dictation":
        return null;
      case "seogi_forget_dictations": {
        const ids = args.ids as number[] | null;
        const before = recentDictations.length;
        recentDictations = ids
          ? recentDictations.filter((item) => !ids.includes(item.id))
          : [];
        return before - recentDictations.length;
      }
      case "seogi_sign_ins":
        return { ...signIns };
      case "seogi_phone_status":
        return { ...phone };
      case "seogi_phone_connect":
        Object.assign(phone, {
          connected: true,
          account: "someone@example.com",
        });
        return { ...phone };
      case "seogi_phone_disconnect":
        Object.assign(phone, { connected: false, account: "" });
        return { ...phone };
      case "seogi_drive_status":
        return { ...drive, excluded: [...drive.excluded] };
      case "seogi_drive_backlog":
        return {
          meetings: backlogLeft,
          bytes: backlogLeft > 0 ? 3.4 * 1024 ** 3 : 0,
        };
      case "seogi_drive_upload_backlog": {
        const n = backlogLeft;
        backlogLeft = 0;
        drive.waiting += n;
        return n;
      }
      case "seogi_drive_exclude": {
        const id = String(args.id);
        drive.excluded = drive.excluded.filter((item) => item !== id);
        if (args.excluded) drive.excluded.push(id);
        return null;
      }
      case "seogi_sign_in": {
        const tool = args.tool as Tool;
        window.setTimeout(() => (signIns[tool] = "signed_in"), 4000);
        return null;
      }
      case "seogi_open_install_page": {
        const tool = args.tool as Tool;
        window.setTimeout(() => (signIns[tool] = "signed_out"), 4000);
        return null;
      }
      case "seogi_try_summary":
        return { ok: true, ms: 2100, model: "claude sonnet", trouble: "" };
      case "seogi_dictation":
        return { ...dictation };
      case "seogi_dictation_status":
        return dictationStatus();
      case "seogi_set_dictation": {
        const next = args.dictation as Dictation;
        if (next.shortcut === "ctrl+option+r")
          return Promise.reject("shortcut_taken:record");
        Object.assign(dictation, next);
        return null;
      }
      case "seogi_name_speaker": {
        const speaker = done.transcript?.speakers.find(
          (one) => one.id === args.speaker,
        );
        if (speaker && args.scope === "all") speaker.name = String(args.name);
        return null;
      }
      case "seogi_restore_naming": {
        // As the backend does it: who said each line and who was given
        // each task, as kept before the naming.
        const kept = args.kept as Naming;
        if (done.transcript) {
          done.transcript.speakers = kept.speakers.map((one) => ({ ...one }));
          done.transcript.segments.forEach((line, i) => {
            line.speaker = kept.lines[i]?.speaker ?? line.speaker;
          });
        }
        const entries = (done.minutes ?? []).flatMap((m) =>
          m.sections.flatMap((section) => section.entries),
        );
        entries.forEach((entry, i) => {
          entry.owner = kept.tasks[i]?.owner ?? entry.owner;
        });
        return null;
      }
      case "seogi_talk":
        return {
          // Measured from the made-up meeting, as the backend would.
          speakers: talkOf(done),
          moments: [
            { at_ms: 40000, by: 1, from: 3, overlap_ms: 600 },
            { at_ms: 126000, by: 2, from: 3, overlap_ms: 300 },
          ],
        };

      case "seogi_terms_from_documents":
        return {
          terms: ["한빛마을", "새봄마루", "포세린 타일", "걸레받이"],
          skipped: [{ file: "현장 사진.jpg", reason: "pictures_local" }],
        };
      case "seogi_add_to_vocabulary":
        return (args.terms as string[]).length;
      case "plugin:dialog|open":
        return [
          "/Users/me/Documents/견적서.pdf",
          "/Users/me/Documents/현장 사진.jpg",
        ];
      case "seogi_delete_meeting": {
        const at = meetings.findIndex((one) => one.id === args.id);
        if (at >= 0) trashedRows.push(...meetings.splice(at, 1));
        return null;
      }
      case "seogi_undo_delete":
        meetings.push(...trashedRows.splice(0));
        meetings.sort((a, b) => b.started_at_ms - a.started_at_ms);
        return null;
      case "seogi_set_meeting_info": {
        const info = args.info as Record<string, unknown>;
        const target = done as unknown as Record<string, unknown>;
        for (const [field, value] of Object.entries(info)) {
          if (value !== null) target[field] = value;
        }
        return null;
      }
      case "seogi_watch_folder":
        config.watched.push({
          path: String(args.path),
          enabled: true,
          template_id: "",
          since_ms: now,
        });
        return null;
      case "seogi_start_recording":
        isRecording = true;
        void emit("recording-changed", { recording: true });
        return "recording";
      case "seogi_stop_recording":
        isRecording = false;
        void emit("recording-changed", { recording: false });
        return "recording";
      case "seogi_mark":
        recording.marks_ms = [...recording.marks_ms, recording.elapsed_ms];
        return recording.elapsed_ms;
      case "seogi_edit_task": {
        const entry = taskSection(args.section)?.entries[Number(args.index)];
        if (entry) {
          const text = String(args.text).trim();
          const written = entry.original ?? entry.text;
          entry.original = written !== text && !entry.added ? written : null;
          entry.text = text;
        }
        return null;
      }
      case "seogi_remove_task": {
        const section = taskSection(args.section);
        if (!section) return null;
        const index = Number(args.index);
        const [entry] = section.entries.splice(index, 1);
        section.removed = [...(section.removed ?? []), { index, entry }];
        return null;
      }
      case "seogi_restore_task": {
        const section = taskSection(args.section);
        const removed = section?.removed ?? [];
        const [back] = removed.splice(Number(args.removed), 1);
        if (section && back) {
          section.entries.splice(
            Math.min(back.index, section.entries.length),
            0,
            back.entry,
          );
        }
        return null;
      }
      case "seogi_add_task": {
        const section =
          taskSection(args.section) ??
          done.minutes?.[0]?.sections.find((item) => item.kind === "actions");
        section?.entries.push({
          text: String(args.text).trim(),
          owner: "",
          due: "",
          sources: [],
          reasons: [],
          done: false,
          faint: false,
          original: null,
          added: true,
          id: `added-${Date.now()}`,
        });
        return null;
      }
      case "seogi_word_places":
        return Math.max(
          1,
          fixPlaces(Number(args.atMs), Number(args.at), String(args.from))
            .length,
        );
      case "seogi_fix_word": {
        const from = String(args.from);
        const to = String(args.to);
        const places = fixPlaces(Number(args.atMs), Number(args.at), from);
        const before: Segment[] = [];
        for (const place of places) {
          if (!before.some((line) => line.start_ms === place.line.start_ms))
            before.push(structuredClone(place.line));
          place.line.fixes = [
            ...(place.line.fixes ?? []),
            { at: place.at, from, to, auto: false },
          ];
          refix(place.line);
        }
        let remembered = false;
        const known = vocabularyWords.some(
          (word) =>
            word.term === from || word.misheard.some((m) => m.form === from),
        );
        if (args.remember && Array.from(from).length >= 2 && !known) {
          const word = vocabularyWords.find((one) => one.term === to);
          const learnt = {
            form: from,
            added_on: todayIso,
            meeting: done.title,
          };
          if (word) word.misheard = [...word.misheard, learnt];
          else
            vocabularyWords.unshift({
              term: to,
              misheard: [learnt],
              editable: true,
            });
          remembered = true;
        }
        return { places: places.length, before, remembered };
      }
      case "seogi_unfix_words": {
        for (const kept of args.before as Segment[]) {
          const line = done.transcript?.segments.find(
            (one) => one.start_ms === kept.start_ms,
          );
          if (line) {
            line.edited = kept.edited;
            line.fixes = kept.fixes;
          }
        }
        if (args.forget) {
          for (const word of vocabularyWords) {
            if (word.term === args.to)
              word.misheard = word.misheard.filter((m) => m.form !== args.from);
          }
        }
        return null;
      }
      case "seogi_unfix_word": {
        for (const line of done.transcript?.segments ?? []) {
          if (
            line.start_ms < Number(args.fromMs) ||
            line.start_ms > Number(args.toMs)
          )
            continue;
          const fixes = line.fixes ?? [];
          const mine = (fix: WordFix) =>
            fix.from === args.from && fix.to === args.to;
          if (!fixes.some(mine)) continue;
          // The vocabulary's fix is in the words as transcribed: the heard
          // words come back as the user's fix over them.
          line.fixes = fixes.flatMap((fix) =>
            !mine(fix)
              ? [fix]
              : fix.auto
                ? [{ at: fix.at, from: fix.to, to: fix.from, auto: false }]
                : [],
          );
          refix(line);
        }
        return null;
      }
      case "seogi_vocabulary":
        return { words: structuredClone(vocabularyWords), writable: true };
      case "seogi_save_word": {
        const term = String(args.term).trim();
        const misheard = (args.misheard as Misheard[]).map((m) => ({
          ...m,
          added_on: m.added_on || todayIso,
        }));
        const old = args.old === null ? term : String(args.old);
        const at = vocabularyWords.findIndex((word) => word.term === old);
        const word = { term, misheard, editable: true };
        if (at >= 0) vocabularyWords[at] = word;
        else vocabularyWords.push(word);
        return null;
      }
      case "seogi_remove_word":
        vocabularyWords = vocabularyWords.filter(
          (word) => word.term !== args.term,
        );
        return null;
      case "seogi_edit_lines": {
        // Simpler than the backend's: the paragraph's words go to its first
        // line, unless they are the words transcribed.
        const segments = done.transcript?.segments ?? [];
        const first = segments.findIndex(
          (line) => line.start_ms === Number(args.fromMs),
        );
        if (first < 0) throw new Error("no line starts there");
        let last = first;
        while (
          segments[last + 1] &&
          segments[last + 1].speaker === segments[first].speaker &&
          segments[last + 1].end_ms <= Number(args.toMs)
        )
          last += 1;
        const lines = segments.slice(first, last + 1);
        const text = String(args.text).trim();
        const asTranscribed = lines.map((line) => line.text.trim()).join(" ");
        lines.forEach((line, index) => {
          line.edited = text === asTranscribed ? null : index === 0 ? text : "";
          line.fixes = [];
        });
        return null;
      }
      case "seogi_set_action_done": {
        const section = done.minutes?.[0]?.sections.find(
          (item) => item.key === args.section,
        );
        const entry = section?.entries[Number(args.index)];
        if (entry) entry.done = Boolean(args.done);
        return null;
      }
      case "get_app_settings":
        return { app_language: "ko", autostart_enabled: atLogin };
      case "change_autostart_setting":
        atLogin = Boolean(args.enabled);
        return null;
      case "seogi_login_item":
        // `?loginItem=needs_approval` shows it switched off in System Settings.
        return (
          new URLSearchParams(window.location.search).get("loginItem") ??
          (atLogin ? "on" : "off")
        );
      case "seogi_open_login_items":
        return null;
      case "seogi_download_dictation_model":
        return null;
      case "seogi_config":
        return config;
      case "seogi_templates":
        return templates;
      case "seogi_readiness":
        return readinessNow();
      case "seogi_meetings":
        return isRecording
          ? [
              summary({
                id: "recording",
                status: "recording",
                folder:
                  "/Users/me/Documents/서기/2026-11-24 1400 김서윤 초도 상담",
              }),
              ...meetings,
            ]
          : [...meetings];
      case "seogi_save_summary_card":
        (window as unknown as { __savedCard: unknown }).__savedCard = {
          kind: args.kind,
          data: args.data,
        };
        return `/Users/me/Documents/서기/${done.id}/한 장 요약.${String(args.kind)}`;
      case "seogi_copy_image":
        return null;
      case "seogi_meeting":
        if (args.id === "recording") return recordingMeeting;
        // A fresh copy each time, as the backend gives.
        if (args.id === done.id) return structuredClone(done);
        throw new Error("no such meeting in the preview");
      case "seogi_recording":
        return isRecording ? recording : null;
      case "seogi_today":
        // A fresh copy each time, as the backend gives.
        return structuredClone(
          screen === "home-going"
            ? going
            : screen === "home-going-long"
              ? goingLong
              : screen === "home-many"
                ? many
                : upcoming,
        );
      case "seogi_upcoming":
        return [...upcoming, ...going, ...many].map((shown) => shown.event);
      case "seogi_mute_event": {
        // Turned off: the next one takes its place, as in the app.
        const off = args.muted === true;
        for (const list of [upcoming, going, many]) {
          for (const shown of list) {
            if (shown.event.uid === args.uid) shown.muted = off;
          }
        }
        config.muted_events = config.muted_events.filter(
          (uid) => uid !== args.uid,
        );
        if (off) config.muted_events.push(String(args.uid));
        return null;
      }
      case "seogi_banner":
        if (screen === "banner-starting") {
          return { kind: "starting", event: upcoming[0].event };
        }
        if (screen === "banner-ready") {
          return {
            kind: "ready",
            id: done.id,
            title: "[미팅] 새봄마루 / 원목마루 단가",
            actions: 4,
            to_check: 1,
          };
        }
        return null;
      case "seogi_search":
        return search(String(args.query));
      case "seogi_microphones":
        return [];
      case "seogi_has_key":
        return savedKeys.has(String(args.name));
      case "seogi_set_key":
        savedKeys.add(String(args.name));
        return null;
      case "seogi_remove_key":
        savedKeys.delete(String(args.name));
        return null;
      case "seogi_key_models":
        return [
          "claude-sonnet-5-5",
          "claude-opus-5-5",
          "claude-haiku-5-5",
          "claude-fable-5-2",
        ];
      case "seogi_ask": {
        const source = done.minutes?.[0]?.sections[0]?.entries[0]?.sources[0];
        return {
          text: "신발장 도면을 다시 그려 금요일까지 보내기로 했습니다.",
          found: true,
          sources: source ? [source] : [],
          unsupported: false,
        };
      }
      case "plugin:macos-permissions|check_microphone_permission":
        return micAllowed;
      case "plugin:macos-permissions|check_accessibility_permission":
        return dictationStatus().permitted;
      case "plugin:macos-permissions|request_microphone_permission":
        micAllowed = true;
        return null;
      case "seogi_try_notification":
        return null;
      case "get_windows_microphone_permission_status":
        return {
          supported: true,
          app_access: "allowed",
          desktop_app_access: "allowed",
          device_access: "allowed",
          overall_access: micAllowed ? "allowed" : "denied",
        };
      case "seogi_check_calendar": {
        const source = String(args.source ?? "").trim();
        const good = source.endsWith(".ics");
        // A real calendar takes a moment to answer over the network.
        return new Promise((resolve) => window.setTimeout(resolve, 800)).then(
          () => ({
            ok: good,
            today: good ? upcoming.length : 0,
            error: good ? "" : "could not read the calendar file",
            reason: good ? "" : "not_address",
            address: source.replace(/^webcal:\/\//i, "https://"),
            next: good ? upcoming[0].event : null,
            read_at_ms: now,
          }),
        );
      }
      case "seogi_calendar_status": {
        const read = {
          ok: true,
          today: upcoming.length,
          error: "",
          reason: "",
          address: "",
          next: upcoming[0].event,
          read_at_ms: now,
        };
        return {
          mac: config.mac_calendars.length > 0 ? read : null,
          addresses: config.calendar_urls.map(() => read),
        };
      }
      case "get_active_notices":
        // `?permission-lost` shows the bands for lost permissions.
        return new URLSearchParams(window.location.search).has(
          "permission-lost",
        )
          ? [
              {
                code: "CalendarPermissionLost",
                level: "error",
                source: "calendar",
                detail: "not_determined",
              },
              {
                code: "TypingPermissionLost",
                level: "error",
                source: "typing",
                detail: null,
              },
            ]
          : [];
      case "plugin:updater|check":
        // `?update` shows a new version on offer.
        return new URLSearchParams(window.location.search).has("update")
          ? {
              rid: 1,
              currentVersion: "0.1.0",
              version: "0.1.1",
              date: new Date(now).toISOString(),
              body: "- 회의록 모델을 목록에서 고릅니다.\n- 맥 캘린더 앱의 일정을 읽습니다.\n- 그 밖의 고친 점",
              rawJson: {},
            }
          : null;
      case "seogi_mac_calendar_access":
        return macAccess;
      case "seogi_mac_calendar_request":
        macAccess = "granted";
        return macAccess;
      case "seogi_mac_calendars":
        return [
          { id: "work", title: "업무", account: "me@work.example" },
          { id: "home", title: "개인", account: "iCloud" },
        ];
      default:
        return null;
    }
  },
  { shouldMockEvents: true },
);

async function show() {
  const root = ReactDOM.createRoot(
    document.getElementById("root") as HTMLElement,
  );
  if (screen.startsWith("indicator-")) {
    await import("../i18n");
    const { default: RecordingOverlay } = await import(
      "../../overlay/RecordingOverlay"
    );
    document.body.style.background = "#d9dbdf";
    root.render(
      <div style={{ width: 440, height: 50, position: "relative" }}>
        <RecordingOverlay />
      </div>,
    );
    const kind = screen.slice(10);
    window.setTimeout(() => {
      if (kind === "listening") {
        void emit("show-overlay", "recording");
        void emit("recording-ready", null);
        window.setInterval(
          () =>
            void emit(
              "mic-level",
              Array.from({ length: 16 }, () => 0.2 + Math.random() * 0.7),
            ),
          80,
        );
      } else if (kind === "tidying") {
        void emit("show-overlay", "processing");
      } else if (kind === "nowhere") {
        void emit("overlay-note", "nowhere");
        void emit("show-overlay", "notice");
      } else {
        void emit("overlay-note", "tidyLimit");
        void emit("show-overlay", "notice");
      }
    }, 300);
    return;
  }
  if (screen.startsWith("banner")) {
    await import("../i18n");
    await import("../seogi.css");
    const { default: Banner } = await import("../banner/Banner");
    document.body.style.background = "#d9dbdf";
    root.render(
      <div style={{ width: screen === "banner-starting" ? 584 : 312 }}>
        <Banner />
      </div>,
    );
    return;
  }
  const { default: SeogiApp } = await import("../App");
  const { useSeogi } = await import("../store");
  root.render(
    <React.StrictMode>
      <SeogiApp />
    </React.StrictMode>,
  );
  // Go to the screen asked for once the first load is in.
  const stop = useSeogi.subscribe((state) => {
    if (!state.loaded) return;
    stop();
    if (screen === "minutes" || screen.startsWith("minutes-"))
      state.open(done.id);
    if (screen === "minutes-editing") {
      // Press "고치기" on the second task and on the third transcript line
      // once they are on the screen.
      const press = window.setInterval(() => {
        const task = document.querySelector<HTMLElement>(
          '[data-task="1"] .acts button',
        );
        if (!task) return;
        window.clearInterval(press);
        task.click();
        const details = Array.from(
          document.querySelectorAll<HTMLDetailsElement>("details"),
        );
        for (const fold of details) fold.open = true;
        window.setTimeout(() => {
          document
            .querySelectorAll<HTMLElement>(".tline .acts button")[2]
            ?.click();
        }, 100);
      }, 50);
    }
    if (screen.startsWith("card")) {
      state.open(done.id);
      // Press "한 장 요약" once the minutes are on the screen.
      const press = window.setInterval(() => {
        const key = document.querySelector<HTMLElement>("[data-card-open]");
        if (!key) return;
        window.clearInterval(press);
        key.click();
      }, 50);
    }
    if (screen === "onboarding") {
      // `&step=2` or `&step=3`: press "다음" that many times less one.
      let presses = Number(query.get("step") ?? 1) - 1;
      const press = window.setInterval(() => {
        const key = document.querySelector<HTMLElement>("[data-step-next]");
        if (presses <= 0 || !key) {
          if (presses <= 0) window.clearInterval(press);
          return;
        }
        presses -= 1;
        key.click();
      }, 50);
    }
    if (screen === "settings") state.go({ name: "settings" });
    if (screen === "settings-dictation")
      state.go({ name: "settings", part: "dictation" });
    if (screen === "settings-vocabulary") {
      state.go({ name: "settings", part: "recording" });
      // Press the vocabulary's "열기" once the settings are on the screen.
      const press = window.setInterval(() => {
        const key = document.querySelector<HTMLElement>(
          'button[aria-label="용어집 열기"]',
        );
        if (!key) return;
        window.clearInterval(press);
        key.click();
      }, 50);
    }
    if (screen === "waiting") state.select("working");
  });
}

void show();
