// What the screens share: the meetings, the recording in progress, the
// settings, and which screen is showing. Everything comes from the backend
// through the seogi* commands and events; nothing is decided here.

import { create } from "zustand";
import { microphoneAllowed } from "./lib/microphone";
import { downloadAsked, downloadEnded, noSpace } from "./lib/speech";
import {
  commands,
  events,
  type Config,
  type Event as CalendarEvent,
  type Found,
  type FoundPlace,
  type Meeting,
  type MeetingSummary,
  type Readiness,
  type RecordingState,
  type Template,
  type Upcoming,
} from "@/bindings";

/** What `startRecording` returns when the system will not let the app use
 *  the microphone. */
export const MICROPHONE_DENIED = "microphone_denied";

export type View =
  | { name: "home" }
  | { name: "minutes"; id: string }
  | { name: "settings"; part?: SettingsPart };

/** The groups the settings are sorted into, in the order the side list
 *  shows them. A way into the settings may name one to open. */
export const SETTINGS_PARTS = [
  "summary",
  "recording",
  "dictation",
  "keep",
  "start",
] as const;
export type SettingsPart = (typeof SETTINGS_PARTS)[number];

/** Where a search sent the minutes: the line with the words, marked until
 *  the person presses somewhere else, goes back or opens another meeting.
 *  No place: the meeting as a whole, with the words marked in its title. */
export interface Jump {
  id: string;
  words: string;
  place: FoundPlace | null;
}

/** The meetings list's finding. Kept here, not on the home, so that going
 *  back from a found meeting shows the same words and lines to press the
 *  next one. */
export interface Finding {
  open: boolean;
  query: string;
  /** What the backend found, by meeting, for the words it was asked. */
  found: { words: string; hits: Record<string, Found> } | null;
}

export interface ImportTally {
  added: number;
  duplicates: number;
  failed: string[];
}

interface Seogi {
  loaded: boolean;
  config: Config | null;
  readiness: Readiness | null;
  templates: Template[];
  meetings: MeetingSummary[];
  /** The meeting chosen in the list, shown beside it. */
  selected: string | null;
  /** The chosen or opened meeting in full. */
  detail: Meeting | null;
  recording: RecordingState | null;
  today: Upcoming[];
  view: View;
  /** The settings group looked at last, shown again when the settings
   *  open without naming one. */
  settingsPart: SettingsPart;
  /** Where ⌘[ / Alt+← goes, the latest last; and where ⌘] goes back to. */
  backStack: View[];
  forwardStack: View[];
  /** Download progress of models, by model id, 0 to 100. */
  downloads: Record<string, number>;
  /** Whole minutes left of a model's download, once its speed is known. */
  downloadMinutesLeft: Record<string, number>;
  /** Models whose last download failed, until one is tried again. */
  downloadFailed: string[];
  /** Bytes the disk is short of for a model's download. */
  downloadShort: Record<string, number>;
  /** The home's band about the speech model was put off until the app is
   *  started again. */
  speechNoticeLater: boolean;
  /** Documents dropped on an open meeting, for its words. */
  droppedDocuments: { id: string; paths: string[] } | null;
  /** The meeting just moved to the trash, which can be brought back. */
  trashed: { title: string; at: number } | null;
  jump: Jump | null;
  finding: Finding;

  init: () => Promise<void>;
  refreshMeetings: () => Promise<void>;
  refreshSetup: () => Promise<void>;
  refreshToday: () => Promise<void>;
  refreshRecording: () => Promise<void>;
  refreshDetail: () => Promise<void>;
  select: (id: string | null) => void;
  /** Opens a meeting's minutes; from a search, at the line it found. */
  open: (id: string, found?: Omit<Jump, "id">) => void;
  /** Takes the search's highlighter off the minutes. */
  clearJump: () => void;
  /** Changes the finding; closing it forgets the words. */
  setFinding: (change: Partial<Finding>) => void;
  go: (view: View) => void;
  setSettingsPart: (part: SettingsPart) => void;
  goBack: () => void;
  goForward: () => void;
  saveConfig: (config: Config) => Promise<string | null>;
  startRecording: (event?: CalendarEvent) => Promise<string | null>;
  stopRecording: () => Promise<string | null>;
  importPaths: (paths: string[]) => Promise<ImportTally>;
  dropDocuments: (dropped: { id: string; paths: string[] } | null) => void;
  /** Downloads a speech model for meetings, once, if the disk has room. */
  downloadSpeechModel: (model: string) => Promise<void>;
  putOffSpeechNotice: () => void;
}

let started = false;

export const useSeogi = create<Seogi>((set, get) => ({
  loaded: false,
  config: null,
  readiness: null,
  templates: [],
  meetings: [],
  selected: null,
  detail: null,
  recording: null,
  today: [],
  view: { name: "home" },
  settingsPart: "summary",
  backStack: [],
  forwardStack: [],
  droppedDocuments: null,
  trashed: null,
  jump: null,
  finding: { open: false, query: "", found: null },
  downloads: {},
  downloadMinutesLeft: {},
  downloadFailed: [],
  downloadShort: {},
  speechNoticeLater: false,

  init: async () => {
    if (started) return;
    started = true;
    void events.meetingChanged.listen((event) => {
      void get().refreshMeetings();
      const { selected, view } = get();
      const shown = view.name === "minutes" ? view.id : selected;
      if (event.payload.id === shown) void get().refreshDetail();
    });
    void events.recordingChanged.listen(() => void get().refreshRecording());
    void events.meetingStarting.listen(() => void get().refreshToday());
    void events.openMeeting.listen((event) => get().open(event.payload.id));

    const [config, templates] = await Promise.all([
      commands.seogiConfig(),
      commands.seogiTemplates(),
    ]);
    set({ config, templates });
    await Promise.all([
      get().refreshMeetings(),
      get().refreshRecording(),
      get().refreshToday(),
    ]);
    const first = get().meetings[0];
    if (first) get().select(first.id);
    set({ loaded: true });
    void get().refreshSetup();
  },

  refreshMeetings: async () => {
    set({ meetings: await commands.seogiMeetings() });
  },

  refreshSetup: async () => {
    const [config, readiness] = await Promise.all([
      commands.seogiConfig(),
      commands.seogiReadiness(),
    ]);
    set({
      config,
      readiness: readiness.status === "ok" ? readiness.data : get().readiness,
    });
  },

  refreshToday: async () => {
    set({ today: await commands.seogiToday() });
  },

  refreshRecording: async () => {
    const result = await commands.seogiRecording();
    if (result.status !== "ok") return;
    const before = get().recording;
    set({ recording: result.data });
    // A recording that has just ended is the meeting to look at.
    if (before && !result.data) {
      await get().refreshMeetings();
      get().select(before.id);
      set({ view: { name: "home" } });
    }
  },

  refreshDetail: async () => {
    const { selected, view } = get();
    const id = view.name === "minutes" ? view.id : selected;
    if (!id) {
      set({ detail: null });
      return;
    }
    const result = await commands.seogiMeeting(id);
    const still = get();
    const shown =
      still.view.name === "minutes" ? still.view.id : still.selected;
    if (shown === id) {
      set({ detail: result.status === "ok" ? result.data : null });
    }
  },

  select: (id) => {
    if (get().selected !== id) set({ selected: id, detail: null });
    void get().refreshDetail();
  },

  open: (id, found) => {
    if (get().detail?.id !== id) set({ detail: null });
    set({
      selected: id,
      jump: found ? { id, ...found } : null,
      ...moved(get(), { name: "minutes", id }),
    });
    void get().refreshDetail();
  },

  clearJump: () => {
    if (get().jump) set({ jump: null });
  },

  setFinding: (change) => {
    const next = { ...get().finding, ...change };
    set({
      finding: next.open ? next : { open: false, query: "", found: null },
    });
  },

  go: (view) => {
    set({ ...moved(get(), view), jump: null });
    arrived(view);
  },

  setSettingsPart: (part) => set({ settingsPart: part }),

  goBack: () => {
    const { backStack, forwardStack, view, meetings } = get();
    const step = stepped(backStack, forwardStack, view, meetings);
    if (!step) return;
    set({
      view: step.to,
      backStack: step.from,
      forwardStack: step.onto,
      jump: null,
    });
    arrived(step.to);
  },

  goForward: () => {
    const { backStack, forwardStack, view, meetings } = get();
    const step = stepped(forwardStack, backStack, view, meetings);
    if (!step) return;
    set({
      view: step.to,
      forwardStack: step.from,
      backStack: step.onto,
      jump: null,
    });
    arrived(step.to);
  },

  downloadSpeechModel: async (model) => {
    if (get().downloads[model] !== undefined) return;
    set((state) => downloadAsked(state, model));
    const result = await commands.seogiDownloadSpeechModel(model);
    if (result.status === "error") {
      set((state) => downloadEnded(state, model, true, noSpace(result.error)));
    }
    void get().refreshSetup();
  },

  putOffSpeechNotice: () => set({ speechNoticeLater: true }),

  saveConfig: async (config) => {
    const result = await commands.seogiSetConfig(config);
    if (result.status === "error") return result.error;
    set({ config });
    void get().refreshSetup();
    void get().refreshToday();
    return null;
  },

  startRecording: async (event) => {
    if (!(await microphoneAllowed())) return MICROPHONE_DENIED;
    const result = await commands.seogiStartRecording(
      null,
      event?.uid ?? null,
      event?.start_ms ?? null,
    );
    await get().refreshRecording();
    return result.status === "error" ? result.error : null;
  },

  stopRecording: async () => {
    const result = await commands.seogiStopRecording();
    await get().refreshRecording();
    return result.status === "error" ? result.error : null;
  },

  dropDocuments: (dropped) => set({ droppedDocuments: dropped }),

  importPaths: async (paths) => {
    const tally: ImportTally = { added: 0, duplicates: 0, failed: [] };
    if (paths.length === 0) return tally;
    const result = await commands.seogiImport(paths);
    if (result.status !== "ok") {
      tally.failed.push(result.error);
      return tally;
    }
    for (const file of result.data) {
      if (file.error) tally.failed.push(file.path);
      else if (file.duplicate) tally.duplicates += 1;
      else tally.added += 1;
    }
    await get().refreshMeetings();
    const newest = result.data.find((file) => file.id);
    if (newest) get().select(newest.id);
    return tally;
  },
}));

function sameView(a: View, b: View): boolean {
  return (
    a.name === b.name &&
    (a.name !== "minutes" || (b.name === "minutes" && a.id === b.id))
  );
}

/** The new screen, with the one left behind remembered for going back. */
function moved(
  state: Pick<Seogi, "view" | "backStack">,
  view: View,
): Pick<Seogi, "view" | "backStack" | "forwardStack"> | { view: View } {
  if (sameView(state.view, view)) return { view };
  return {
    view,
    backStack: [...state.backStack, state.view].slice(-30),
    forwardStack: [],
  };
}

/** Whether a screen can still be shown: a meeting's minutes only while the
 *  meeting is in the list (not moved to the trash). */
function still(view: View, meetings: MeetingSummary[]): boolean {
  return view.name !== "minutes" || meetings.some((m) => m.id === view.id);
}

/** One step back (or forward) through `from`, past meetings that are gone;
 *  the screen left behind goes onto `onto`. `null` when nothing is left. */
function stepped(
  from: View[],
  onto: View[],
  view: View,
  meetings: MeetingSummary[],
): { to: View; from: View[]; onto: View[] } | null {
  const left = [...from];
  let to = left.pop();
  while (to && !still(to, meetings)) to = left.pop();
  if (!to) return null;
  return {
    to,
    from: left,
    onto: still(view, meetings) ? [...onto, view] : onto,
  };
}

/** What a screen needs when it comes up. */
function arrived(view: View) {
  const state = useSeogi.getState();
  if (view.name === "home") void state.refreshDetail();
  if (view.name === "minutes") {
    useSeogi.setState({ selected: view.id });
    void state.refreshDetail();
  }
  if (view.name === "settings") {
    if (view.part) useSeogi.setState({ settingsPart: view.part });
    void state.refreshSetup();
  }
}

/** The folder of a meeting, from the list. */
export function folderOf(id: string): string | null {
  return useSeogi.getState().meetings.find((m) => m.id === id)?.folder ?? null;
}
