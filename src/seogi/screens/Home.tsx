// The home: one quiet column. The record key and the next event on one
// line, the meetings below it by day. Finding, bringing in files and the
// settings wait behind three icons at the top right. Pressing a meeting
// opens its minutes.

import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { platform } from "@tauri-apps/plugin-os";
import {
  commands,
  type Found,
  type MeetingSummary,
  type Upcoming,
} from "@/bindings";
import { useLength, useNow } from "../components/common";
import { Marked } from "../components/Found";
import { around, has } from "../lib/found";
import {
  CloseIcon,
  ImportIcon,
  SearchIcon,
  SlidersIcon,
} from "../components/icons";
import UpdateNotice from "../components/UpdateNotice";
import PermissionNotice from "../components/PermissionNotice";
import { ConfirmDelete, busy } from "../components/DeleteMeeting";
import { NS } from "../i18n";
import { clock, daysAgo, fullDay, hm, keysShort, longDay } from "../lib/format";
import { pick } from "../lib/upcoming";
import { MICROPHONE_DENIED, useSeogi } from "../store";

const RECORDING_KINDS = [
  "m4a",
  "mp3",
  "wav",
  "aac",
  "3gp",
  "3ga",
  "amr",
  "ogg",
  "opus",
  "flac",
  "wma",
  "mp4",
  "mov",
  "webm",
  "caf",
  "aiff",
];

type Filter = "all" | "consult" | "vendor" | "meeting";
const FILTERS: Filter[] = ["all", "consult", "vendor", "meeting"];

/** Asks the home to open finding (편집 › 찾기 in the menu bar). */
export const FIND_EVENT = "seogi-find";

export default function Home() {
  const { t } = useTranslation(NS);
  const go = useSeogi((s) => s.go);
  const importPaths = useSeogi((s) => s.importPaths);
  const finding = useSeogi((s) => s.finding.open);
  const setFinding = useSeogi((s) => s.setFinding);
  const [note, setNote] = useState("");
  // Something that did not work is shown so that it cannot be missed.
  const [bad, setBad] = useState(false);
  const mac = platform() === "macos";

  const warn = (text: string) => {
    setBad(true);
    setNote(text);
  };

  // ⌘K or ⌘F (Ctrl+K, Ctrl+F) finds, as in most apps; so does 편집 › 찾기.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      if ((event.metaKey || event.ctrlKey) && (key === "k" || key === "f")) {
        event.preventDefault();
        setFinding({ open: true });
      }
      if (event.key === "Escape") setFinding({ open: false });
    };
    const onFind = () => setFinding({ open: true });
    window.addEventListener("keydown", onKey);
    window.addEventListener(FIND_EVENT, onFind);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener(FIND_EVENT, onFind);
    };
  }, []);

  const bringIn = async (paths: string[]) => {
    const tally = await importPaths(paths);
    const parts: string[] = [];
    if (tally.added > 0 || tally.duplicates > 0) {
      parts.push(
        tally.duplicates > 0
          ? t("home.importedSome", { n: tally.added, dup: tally.duplicates })
          : t("home.imported", { n: tally.added }),
      );
    }
    for (const name of tally.failed) {
      parts.push(
        t("home.importFailed", { name: name.split(/[\\/]/).pop() ?? name }),
      );
    }
    setBad(tally.failed.length > 0);
    setNote(parts.join(" "));
  };

  const pickFiles = async () => {
    const picked = await open({
      multiple: true,
      filters: [{ name: t("home.box"), extensions: RECORDING_KINDS }],
    });
    if (picked) await bringIn(Array.isArray(picked) ? picked : [picked]);
  };

  return (
    <div className="min-h-full pb-16">
      <header className="a-bar">
        <span className="wm">{t("brand")}</span>
        <span className="tools">
          <button
            type="button"
            className="ib"
            aria-pressed={finding}
            aria-label={t("band.find")}
            onClick={() => setFinding({ open: !finding })}
          >
            <SearchIcon />
            <span className="t3">{mac ? "⌘K" : "Ctrl+K"}</span>
          </button>
          <button
            type="button"
            className="ib"
            aria-label={t("band.importFiles")}
            title={t("band.importFiles")}
            onClick={() => void pickFiles()}
          >
            <ImportIcon />
          </button>
          <button
            type="button"
            className="ib"
            aria-label={t("band.settings")}
            title={t("band.settings")}
            onClick={() => go({ name: "settings" })}
          >
            <SlidersIcon />
          </button>
        </span>
      </header>

      <main className="a-col">
        <RecordLine onNote={warn} />
        <UpdateNotice />
        <PermissionNotice />
        {note ? (
          <p
            role={bad ? "alert" : "status"}
            className={`m-0 mb-2 px-3 text-[13.5px] ${bad ? "rt" : "t2"}`}
          >
            {note}
          </p>
        ) : null}
        <MeetingList
          finding={finding}
          onCloseFind={() => setFinding({ open: false })}
        />
      </main>
    </div>
  );
}

/** How many days after today `ms` is, on this computer's calendar; 0 for
 *  today. */
function later(ms: number, now: number): number {
  const day = (time: number) => {
    const d = new Date(time);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  return Math.round((day(ms) - day(now)) / 86_400_000);
}

/** The record key, and the next event beside it. */
function RecordLine({ onNote }: { onNote: (note: string) => void }) {
  const { t, i18n } = useTranslation(NS);
  const now = useNow(30000);
  const config = useSeogi((s) => s.config);
  const today = useSeogi((s) => s.today);
  const startRecording = useSeogi((s) => s.startRecording);
  const refreshToday = useSeogi((s) => s.refreshToday);
  const go = useSeogi((s) => s.go);
  const recording = useSeogi((s) => s.recording);
  const mac = platform() === "macos";
  const [menu, setMenu] = useState<{
    upcoming: Upcoming;
    x: number;
    y: number;
  } | null>(null);

  useEffect(() => {
    const timer = window.setInterval(() => void refreshToday(), 60000);
    return () => window.clearInterval(timer);
  }, [refreshToday]);

  const record = async (upcoming?: Upcoming) => {
    const error = await startRecording(upcoming?.event);
    if (error === MICROPHONE_DENIED) onNote(t("recording.micDenied"));
    else if (error) onNote(t("failed", { reason: error }));
  };

  const hasCalendar =
    (config?.calendar_urls.length ?? 0) > 0 ||
    (config?.mac_calendars.length ?? 0) > 0;
  const { main: next, going } = pick(today, now, recording !== null);
  const minutes = next ? Math.round((next.event.start_ms - now) / 60000) : 0;

  // The right mouse button on an event offers to turn it off; the next one
  // then takes its place.
  const menuFor = (upcoming: Upcoming) => (event: React.MouseEvent) => {
    event.preventDefault();
    setMenu({ upcoming, x: event.clientX, y: event.clientY });
  };
  const mute = async (upcoming: Upcoming) => {
    setMenu(null);
    const result = await commands.seogiMuteEvent(upcoming.event.uid, true);
    if (result.status === "error")
      onNote(t("failed", { reason: result.error }));
    await refreshToday();
  };

  return (
    <div className="a-rec">
      <button
        type="button"
        className="rec"
        style={{ width: 46, height: 46 }}
        aria-label={t("band.record")}
        onClick={() => void record()}
      />
      <button
        type="button"
        className="lbl p-0 border-0 bg-transparent"
        onClick={() => void record()}
      >
        <b>{t("band.record")}</b>
        <span>
          {config
            ? t("home.shortcutAnywhere", {
                keys: keysShort(config.record_shortcut, mac),
              })
            : null}
        </span>
      </button>
      {next && later(next.event.start_ms, now) ? (
        // Not today: what is next, to know; it is recorded when it comes.
        <span className="a-next" onContextMenu={menuFor(next)}>
          <span className="t3">
            {later(next.event.start_ms, now) === 1
              ? t("home.tomorrow")
              : longDay(next.event.start_ms, i18n.language)}
          </span>
          <span>{`${hm(next.event.start_ms)} ${next.event.title}`}</span>
        </span>
      ) : next ? (
        <span className="a-next">
          <button
            type="button"
            className="grid text-right p-0 border-0 bg-transparent"
            title={t("home.recordEvent")}
            onClick={() => void record(next)}
            onContextMenu={menuFor(next)}
          >
            <span className="t3">
              {minutes <= 0
                ? t("band.going")
                : minutes < 60
                  ? t("home.inMinutes", { n: minutes })
                  : minutes % 60 === 0
                    ? t("home.inHours", { h: minutes / 60 })
                    : t("home.inHoursMinutes", {
                        h: Math.floor(minutes / 60),
                        m: minutes % 60,
                      })}
            </span>
            <span>{`${hm(next.event.start_ms)} ${next.event.title}`}</span>
          </button>
          {going ? (
            // Still going, but long begun: kept small under the next one.
            <button
              type="button"
              className="block max-w-[320px] truncate justify-self-end mt-1 p-0 border-0 bg-transparent text-right text-[12px] t3"
              title={t("home.recordEvent")}
              onClick={() => void record(going)}
              onContextMenu={menuFor(going)}
            >
              {`${t("band.going")} · ${hm(going.event.start_ms)} ${going.event.title}`}
            </button>
          ) : null}
        </span>
      ) : (
        <span className="a-next">
          <span className="t3">
            {hasCalendar ? t("band.noEvents") : t("band.noCalendar")}
          </span>
          {hasCalendar ? null : (
            <button
              type="button"
              className="lnk justify-self-end"
              onClick={() => go({ name: "settings", part: "keep" })}
            >
              {t("band.connectCalendar")}
            </button>
          )}
        </span>
      )}
      {menu ? (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          label={menu.upcoming.event.title}
          onClose={() => setMenu(null)}
        >
          <button
            type="button"
            role="menuitem"
            className={MENU_ITEM}
            onClick={() => void mute(menu.upcoming)}
          >
            {t("banner.mute")}
          </button>
        </ContextMenu>
      ) : null}
    </div>
  );
}

function MeetingList({
  finding,
  onCloseFind,
}: {
  finding: boolean;
  onCloseFind: () => void;
}) {
  const { t, i18n } = useTranslation(NS);
  const now = useNow(60000);
  const meetings = useSeogi((s) => s.meetings);
  const selected = useSeogi((s) => s.selected);
  const openMeeting = useSeogi((s) => s.open);
  const [filter, setFilter] = useState<Filter>("all");
  const query = useSeogi((s) => s.finding.query);
  const found = useSeogi((s) => s.finding.found);
  const setFinding = useSeogi((s) => s.setFinding);
  const field = useRef<HTMLInputElement | null>(null);
  const [menu, setMenu] = useState<{
    meeting: MeetingSummary;
    x: number;
    y: number;
  } | null>(null);
  const [deleting, setDeleting] = useState<MeetingSummary | null>(null);

  useEffect(() => {
    if (finding) field.current?.focus();
    else setFilter("all");
  }, [finding]);

  // Titles and summaries are looked through here; transcripts by the
  // backend, a moment after the typing stops.
  useEffect(() => {
    const words = query.trim();
    if (!finding || words === "") {
      setFinding({ found: null });
      return;
    }
    // An answer that comes after the words changed is for old words.
    let current = true;
    const timer = window.setTimeout(() => {
      void commands.seogiSearch(words).then((result) => {
        if (current && result.status === "ok") {
          setFinding({
            found: {
              words,
              hits: Object.fromEntries(result.data.map((hit) => [hit.id, hit])),
            },
          });
        }
      });
    }, 250);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [finding, query, setFinding]);

  const shown = useMemo(() => {
    const words = query.trim();
    return meetings.filter((meeting) => {
      if (filter !== "all" && meeting.template_id !== filter) return false;
      if (!finding || words === "") return true;
      return (
        has(meeting.title, words) ||
        has(meeting.summary, words) ||
        found?.hits[meeting.id] !== undefined
      );
    });
  }, [meetings, filter, finding, query, found]);

  const days = useMemo(() => {
    const groups: { key: string; at: number; rows: MeetingSummary[] }[] = [];
    for (const meeting of shown) {
      const key = new Date(meeting.started_at_ms).toDateString();
      const last = groups[groups.length - 1];
      if (last && last.key === key) last.rows.push(meeting);
      else groups.push({ key, at: meeting.started_at_ms, rows: [meeting] });
    }
    return groups;
  }, [shown]);

  const dayHeading = (at: number) => {
    const ago = daysAgo(at, now);
    if (ago === 0) return t("today");
    if (ago === 1) return t("yesterday");
    return new Date(at).getFullYear() === new Date(now).getFullYear()
      ? longDay(at, i18n.language)
      : fullDay(at, i18n.language);
  };

  return (
    <section aria-label={t("home.box")}>
      {finding ? (
        <div className="px-3 pb-2">
          <div className="flex items-center gap-2">
            <SearchIcon className="w-4 h-4 t3 flex-none" />
            <input
              ref={field}
              type="search"
              value={query}
              onChange={(event) => setFinding({ query: event.target.value })}
              placeholder={t("band.findPlaceholder")}
              aria-label={t("band.find")}
              className="ul flex-1"
            />
            <button
              type="button"
              className="ib"
              aria-label={t("band.findClose")}
              onClick={onCloseFind}
            >
              <CloseIcon />
            </button>
          </div>
          <div className="seg mt-2.5" role="group" aria-label={t("home.box")}>
            {FILTERS.map((name) => (
              <button
                key={name}
                type="button"
                aria-pressed={filter === name}
                onClick={() => setFilter(name)}
              >
                {t(`home.${name}`)}
              </button>
            ))}
          </div>
        </div>
      ) : null}

      {meetings.length === 0 ? (
        <div className="px-3 py-6">
          <p className="m-0 text-[15px] font-medium">{t("home.empty")}</p>
          <p className="mt-1.5 mb-0 max-w-[52ch] t3">{t("home.emptyHow")}</p>
        </div>
      ) : days.length === 0 ? (
        <p className="px-3 py-6 m-0 t3">{t("home.nothingFound")}</p>
      ) : (
        days.map((day) => (
          <div key={day.key}>
            <h2 className="a-grp m-0">{dayHeading(day.at)}</h2>
            {day.rows.map((meeting) => {
              const words = finding ? query.trim() : "";
              const hit = words ? found?.hits[meeting.id] : undefined;
              // The meeting's line goes to the first line the words are in;
              // only the title, or not yet asked, to the top.
              const first =
                hit?.places.find((place) => place.kind !== "title") ??
                hit?.places[0] ??
                null;
              return (
                <div key={meeting.id}>
                  <Row
                    meeting={meeting}
                    on={meeting.id === selected}
                    onOpen={() =>
                      openMeeting(
                        meeting.id,
                        words
                          ? { words: found?.words ?? words, place: first }
                          : undefined,
                      )
                    }
                    onMenu={(x, y) => setMenu({ meeting, x, y })}
                  />
                  {hit && found ? (
                    <Hits
                      hit={hit}
                      words={found.words}
                      onOpen={(place) =>
                        openMeeting(meeting.id, { words: found.words, place })
                      }
                    />
                  ) : null}
                </div>
              );
            })}
          </div>
        ))
      )}
      {menu ? (
        <RowMenu
          x={menu.x}
          y={menu.y}
          meeting={menu.meeting}
          onClose={() => setMenu(null)}
          onOpen={() => {
            openMeeting(menu.meeting.id);
            setMenu(null);
          }}
          onDelete={() => {
            setDeleting(menu.meeting);
            setMenu(null);
          }}
        />
      ) : null}
      {deleting ? (
        <ConfirmDelete meeting={deleting} onClose={() => setDeleting(null)} />
      ) : null}
    </section>
  );
}

const MENU_ITEM =
  "w-full flex items-center justify-between gap-4 h-8 px-3 border-0 rounded-[6px] bg-transparent text-left text-[13.5px] hover:bg-s2";

/** A small menu at the pointer, closed by Escape or a click elsewhere. */
function ContextMenu({
  x,
  y,
  label,
  onClose,
  children,
}: {
  x: number;
  y: number;
  label: string;
  onClose: () => void;
  children: React.ReactNode;
}) {
  useEffect(() => {
    const away = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", away);
    return () => window.removeEventListener("keydown", away);
  }, [onClose]);
  return (
    <div
      className="fixed inset-0 z-20"
      onClick={onClose}
      onContextMenu={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <div
        role="menu"
        aria-label={label}
        className="fixed w-[220px] p-1.5 rounded-[10px] bg-s1 shadow-[0_8px_24px_var(--sh)]"
        style={{
          left: Math.min(x, window.innerWidth - 230),
          top: Math.min(y, window.innerHeight - 100),
        }}
        onClick={(event) => event.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}

/** The menu a meeting's line opens with the right mouse button. */
function RowMenu({
  x,
  y,
  meeting,
  onClose,
  onOpen,
  onDelete,
}: {
  x: number;
  y: number;
  meeting: MeetingSummary;
  onClose: () => void;
  onOpen: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation(NS);
  return (
    <ContextMenu
      x={x}
      y={y}
      label={meeting.title || t("untitled")}
      onClose={onClose}
    >
      <button
        type="button"
        role="menuitem"
        className={MENU_ITEM}
        onClick={onOpen}
      >
        {t("trash.open")}
      </button>
      <button
        type="button"
        role="menuitem"
        className={MENU_ITEM}
        onClick={onDelete}
      >
        <span>{t("trash.menu")}</span>
        {busy(meeting) ? (
          <span className="text-[12px] t3">{t("trash.busyShort")}</span>
        ) : null}
      </button>
    </ContextMenu>
  );
}

/** Up to three lines of a found meeting with the words in them, each one a
 *  way into the minutes at that line; how many more there are. */
function Hits({
  hit,
  words,
  onOpen,
}: {
  hit: Found;
  words: string;
  onOpen: (place: Found["places"][number]) => void;
}) {
  const { t } = useTranslation(NS);
  const lines = hit.places.filter((place) => place.kind !== "title");
  if (lines.length === 0) return null;
  const shown = lines.slice(0, 3);
  const titled = hit.places.some((place) => place.kind === "title") ? 1 : 0;
  const more = hit.total - titled - shown.length;
  return (
    <div className="a-hits">
      {shown.map((place) => (
        <button
          key={`${place.kind}-${place.section}-${place.index}`}
          type="button"
          className="a-hit"
          onClick={() => onOpen(place)}
        >
          <span className="at">
            {place.kind === "transcript" && place.ms !== null
              ? clock(place.ms)
              : place.kind === "people"
                ? t("people.label")
                : t("home.foundInMinutes")}
          </span>
          <span className="tx">
            <Marked
              text={around(place.text, words)}
              words={words}
              className="hl-soft"
            />
          </span>
        </button>
      ))}
      {more > 0 ? (
        <span className="more">{t("home.foundMore", { n: more })}</span>
      ) : null}
    </div>
  );
}

function Row({
  meeting,
  on,
  onOpen,
  onMenu,
}: {
  meeting: MeetingSummary;
  on: boolean;
  onOpen: () => void;
  onMenu: (x: number, y: number) => void;
}) {
  const { t } = useTranslation(NS);
  const length = useLength();
  const from =
    meeting.origin === "watched"
      ? t("home.fromInbox")
      : meeting.origin === "imported"
        ? t("home.fromFile")
        : "";
  const sub = [meeting.duration_ms > 0 ? length(meeting.duration_ms) : "", from]
    .filter(Boolean)
    .join(", ");
  return (
    <button
      type="button"
      className={`a-row ${on ? "on" : ""}`}
      aria-current={on ? "true" : undefined}
      onClick={onOpen}
      onContextMenu={(event) => {
        event.preventDefault();
        onMenu(event.clientX, event.clientY);
      }}
    >
      <span className="tm">{hm(meeting.started_at_ms)}</span>
      <span className="ti">
        <b>{meeting.title || t("untitled")}</b>
        {sub ? <span>{sub}</span> : null}
      </span>
      <Status meeting={meeting} />
    </button>
  );
}

function Status({ meeting }: { meeting: MeetingSummary }) {
  const { t } = useTranslation(NS);
  switch (meeting.status) {
    case "done":
      if (meeting.error === "no_speech") {
        return <span className="st t3">{t("home.noSpeech")}</span>;
      }
      return (
        <span className={`st ${meeting.actions === 0 ? "t3" : ""}`}>
          {meeting.actions === 0
            ? t("home.noTasksShort")
            : t("home.tasks", { n: meeting.actions })}
          {meeting.to_check > 0 ? (
            <em>{t("home.checkShort", { n: meeting.to_check })}</em>
          ) : null}
        </span>
      );
    case "working": {
      const stage = t(`home.stage.${meeting.stage}`, {
        defaultValue: t("home.stage.writing_minutes"),
      });
      return (
        <span className="st grid justify-items-end">
          {meeting.stage === "transcribing"
            ? t("home.stagePercent", { stage, percent: meeting.percent })
            : stage}
          <span className="pbar">
            <i
              style={{
                width: `${Math.max(0, Math.min(100, meeting.percent))}%`,
              }}
            />
          </span>
        </span>
      );
    }
    case "queued":
      return (
        <span className="st t3">
          {meeting.error === "speech_model_missing"
            ? t("home.waitingModel")
            : t("home.queued")}
        </span>
      );
    case "transcribed":
      return <span className="st t3">{t("home.transcribed")}</span>;
    case "recording":
      return (
        <span className="st rt inline-flex items-center gap-2 justify-end">
          <span className="live" aria-hidden="true" />
          {t("home.recording")}
        </span>
      );
    default:
      return <span className="st rt">{t("home.failedShort")}</span>;
  }
}
