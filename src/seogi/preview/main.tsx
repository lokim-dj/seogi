// Seogi's screens in a plain browser, on made-up data, without the backend.
// For looking at a screen while working on it and for taking pictures of
// every screen in one go (scripts/seogi-screens.mjs). Not part of the app.
//
//   http://localhost:1420/src/seogi/preview/index.html?screen=home
//   screen = home | minutes | recording | onboarding | settings |
//            waiting | banner-starting | banner-ready | settings-dictation |
//            mic-idle | mic-listening | mic-working |
//            indicator-listening | indicator-tidying | indicator-note |
//            indicator-nowhere
//   &claude=missing|signed_out|signed_in&codex=… sets where the two
//   subscription tools stand (settings, onboarding).
//   &summary=key starts on '내 API 키' (Anthropic); &key=anthropic has its
//   key saved.

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
  Found,
  FoundPlace,
  Meeting,
  MeetingSummary,
  Readiness,
  RecordingState,
  SignIn,
  SignIns,
  Template,
  Tool,
  Upcoming,
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
  dictation: {
    enabled: screen === "settings-dictation" || screen === "onboarding",
    tidy: screen === "settings-dictation",
    tidy_with: {
      kind: "ollama",
      base_url: "http://localhost:11434",
      model: "gemma3:4b",
    },
    numbers: true,
    button: false,
    button_at: null,
    key_moved: true,
    model_standing_in: false,
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
  button: false,
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
  permitted:
    (screen !== "settings-dictation" || dictation.button) &&
    screen !== "onboarding",
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

const readiness: Readiness = {
  speech_model:
    "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf",
  speech_model_ready: true,
  diarizer_ready: true,
  ffmpeg: true,
  ollama_running: true,
  ollama_models: ["gemma3:12b"],
  claude_cli: true,
  codex_cli: false,
  summary: "claude sonnet",
  summary_ready: true,
  summary_model_refused: false,
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

const recording: RecordingState = {
  id: "recording",
  started_at_ms: now - 23 * minute - 41000,
  elapsed_ms: 23 * minute + 41000,
  level: 0.31,
  marks_ms: [4 * minute + 12000, 12 * minute + 41000, 19 * minute + 5000],
  device: "MacBook Pro 마이크",
  no_sound: false,
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

const templates = [
  meetingTemplate,
  consult,
  vendor,
  call,
  memo,
] as unknown as Template[];

mockWindows("main");
mockConvertFileSrc("macos");
Object.assign(window, {
  __TAURI_OS_PLUGIN_INTERNALS__: {
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

// What the screens asked of the backend, for scripts/seogi-flows.mjs to read.
const calls: { command: string; args: Record<string, unknown> }[] = [];
Object.assign(window, { __seogiCalls: calls });
let isRecording = screen === "recording";
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
      case "seogi_talk":
        return {
          speakers: [
            {
              speaker: 1,
              talk_ms: 120000,
              share: 56,
              turns: 9,
              longest_ms: 24000,
              longest_at_ms: 15000,
              questions: 2,
              cut_ins: 3,
            },
            {
              speaker: 2,
              talk_ms: 70000,
              share: 33,
              turns: 7,
              longest_ms: 14000,
              longest_at_ms: 6000,
              questions: 1,
              cut_ins: 1,
            },
            {
              speaker: 3,
              talk_ms: 22000,
              share: 10.4,
              turns: 4,
              longest_ms: 7000,
              longest_at_ms: 46000,
              questions: 3,
              cut_ins: 0,
            },
            {
              speaker: 4,
              talk_ms: 1000,
              share: 0.6,
              turns: 1,
              longest_ms: 1000,
              longest_at_ms: 90000,
              questions: 0,
              cut_ins: 0,
            },
          ],
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
        return readiness;
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
      case "seogi_meeting":
        if (args.id === "recording") return recordingMeeting;
        // A fresh copy each time, as the backend gives.
        if (args.id === done.id) return structuredClone(done);
        throw new Error("no such meeting in the preview");
      case "seogi_recording":
        return isRecording ? recording : null;
      case "seogi_today":
        return upcoming;
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
        // `?permission-lost` shows the band for a lost typing permission.
        return new URLSearchParams(window.location.search).has(
          "permission-lost",
        )
          ? [
              {
                code: "TypingPermissionLost",
                level: "error",
                source: "dictation",
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
  if (screen.startsWith("mic-")) {
    await import("../i18n");
    await import("../seogi.css");
    const { default: MicButton } = await import("../mic/MicButton");
    document.body.style.background = "#d9dbdf";
    const state = screen.slice(4) as "idle" | "listening" | "working";
    root.render(
      <div style={{ width: 48, height: 48, position: "relative" }}>
        <MicButton initial={state} />
      </div>,
    );
    if (state === "listening") {
      window.setInterval(
        () =>
          void emit(
            "mic-level",
            Array.from({ length: 16 }, () => 0.3 + Math.random() * 0.6),
          ),
        80,
      );
    }
    return;
  }
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
      <div style={{ width: screen === "banner-starting" ? 680 : 324 }}>
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
    if (screen === "minutes") state.open(done.id);
    if (screen === "settings") state.go({ name: "settings" });
    if (screen === "settings-dictation")
      state.go({ name: "settings", part: "dictation" });
    if (screen === "waiting") state.select("working");
  });
}

void show();
