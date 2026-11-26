// A meeting's minutes, read in one column: who does what, how the meeting
// went, and everything else folded under "자세히". Every line points at the
// moment of the recording it rests on; a thin line at the bottom plays it.

import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type Answer,
  type Entry,
  type Meeting,
  type MinutesSection,
  type NameScope,
  type Segment,
  type TalkStats,
  type Template,
  type Transcript as MeetingTranscript,
} from "@/bindings";
import {
  isUnnamed,
  TimeKey,
  useLength,
  useSpeakerName,
} from "../components/common";
import DocumentWords from "../components/DocumentWords";
import { Marked } from "../components/Found";
import { has } from "../lib/found";
import Feedback from "../components/Feedback";
import {
  BackIcon,
  CopyIcon,
  MoreIcon,
  PauseGlyph,
  PlayGlyph,
  RightIcon,
} from "../components/icons";
import MeetingContext, { setMeetingInfo } from "../components/MeetingContext";
import { NamePicker, PeopleLine } from "../components/People";
import { ConfirmDelete, busy } from "../components/DeleteMeeting";
import { SummaryCardDialog } from "../components/SummaryCard";
import { NS } from "../i18n";
import { clock, fullDay, hm, hms, isoDay, shortModel } from "../lib/format";
import { joinPath, usePlayer, type Player } from "../lib/player";
import {
  maybeSamePerson,
  sampleLines,
  shareBar,
  speakerTones,
} from "../lib/speakers";
import { taskLine, titleNeeded } from "../lib/tasks";
import { lateMinutes, startsMidTalk } from "../lib/opening";
import { isGibberish, koreanMeeting } from "../lib/gibberish";
import { Mark } from "../components/Mark";
import { useSeogi, type Jump } from "../store";

const bare = (text: string) => text.replace(/[\s.,!?'"「」…·]/g, "");

/** Whom and where the meeting is about, in one line: "고객 이서진 · 장소
 *  한빛아파트 103동". Empty when neither is known. */
function headLine(meeting: Meeting, t: (key: string) => string): string {
  const parts: string[] = [];
  if (meeting.customer)
    parts.push(`${t("minutes.customer")} ${meeting.customer}`);
  if (meeting.place) parts.push(`${t("minutes.place")} ${meeting.place}`);
  return parts.join(" · ");
}

/** The people of a meeting, whose names a task line may leave out. */
const peopleOf = (meeting: Meeting) => [
  ...(meeting.people ?? []),
  ...(meeting.transcript?.speakers ?? []).map((speaker) => speaker.name),
];

export default function Minutes({ id }: { id: string }) {
  const { t } = useTranslation(NS);
  const detail = useSeogi((s) => s.detail);
  const summary = useSeogi((s) => s.meetings.find((m) => m.id === id));
  const templates = useSeogi((s) => s.templates);
  const go = useSeogi((s) => s.go);
  const jump = useSeogi((s) => s.jump);
  const meeting = detail?.id === id ? detail : null;
  const [copied, setCopied] = useState(false);
  const [menu, setMenu] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [card, setCard] = useState(false);

  const path =
    summary && meeting ? joinPath(summary.folder, meeting.audio_file) : null;
  const player = usePlayer(id, path, meeting?.duration_ms ?? 0);
  const minutes = meeting?.minutes?.[0];
  const tasks = (minutes?.sections ?? []).filter((s) => s.kind === "actions");

  const copyTasks = async () => {
    const groups = tasks.filter((section) => section.entries.length > 0);
    // Whom and where it is about, so the list makes sense pasted anywhere.
    const head = meeting ? headLine(meeting, t) : "";
    const lines = groups.flatMap((section) => [
      ...(titleNeeded(section, groups.length) ? [`${section.title}:`] : []),
      ...section.entries.map((entry) => {
        const { who, text, unowned } = taskLine(
          section,
          entry,
          meeting ? peopleOf(meeting) : [],
        );
        const owner = who ? `${who} – ` : "";
        const mark = unowned ? ` (${t("ownerUnknown")})` : "";
        const due = entry.due || t("dueUnknown");
        const at = entry.sources[0] ? ` (${clock(entry.sources[0].ms)})` : "";
        return `- [${entry.done ? "x" : " "}] ${owner}${text}${mark} – ${due}${at}`;
      }),
    ]);
    await navigator.clipboard.writeText(
      (head ? [head, ...lines] : lines).join("\n"),
    );
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1600);
  };

  return (
    <div className="min-h-full flex flex-col">
      <header className="a-bar sticky top-0 z-10 bg-bg">
        <button
          type="button"
          className="ib !pl-0.5"
          onClick={() => go({ name: "home" })}
        >
          <BackIcon />
          {t("home.box")}
        </button>
        <span className="tools relative">
          {minutes ? (
            <button
              type="button"
              className="ib"
              data-card-open=""
              onClick={() => setCard(true)}
            >
              {t("card.open")}
            </button>
          ) : null}
          {tasks.length > 0 ? (
            <button
              type="button"
              className="ib"
              onClick={() => void copyTasks()}
            >
              <CopyIcon />
              {copied ? t("home.copied") : t("home.copyTasks")}
            </button>
          ) : null}
          <button
            type="button"
            className="ib"
            aria-label={t("minutes.moreActions")}
            aria-expanded={menu}
            onClick={() => setMenu((on) => !on)}
          >
            <MoreIcon />
          </button>
          {menu && meeting ? (
            <MoreMenu
              meeting={meeting}
              templates={templates}
              onClose={() => setMenu(false)}
              onDelete={() => {
                setMenu(false);
                setDeleting(true);
              }}
            />
          ) : null}
          {card && meeting ? (
            <SummaryCardDialog
              meeting={meeting}
              onClose={() => setCard(false)}
            />
          ) : null}
          {deleting && meeting ? (
            <ConfirmDelete
              meeting={{
                id: meeting.id,
                title: meeting.title,
                status: meeting.status,
              }}
              onClose={() => setDeleting(false)}
              onDeleted={() => go({ name: "home" })}
            />
          ) : null}
        </span>
      </header>

      <main className="flex-1 a-col-wide pt-[22px] pb-16">
        {meeting ? (
          <Sheet
            meeting={meeting}
            folder={summary?.folder ?? ""}
            player={player}
            jump={jump?.id === id ? jump : null}
          />
        ) : null}
      </main>

      {meeting ? <PlayerLine player={player} meeting={meeting} /> : null}
    </div>
  );
}

/** What is done to the meeting as a whole, behind the "more" key. */
function MoreMenu({
  meeting,
  templates,
  onClose,
  onDelete,
}: {
  meeting: Meeting;
  templates: Template[];
  onClose: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation(NS);
  const refreshDetail = useSeogi((s) => s.refreshDetail);
  const working = meeting.status === "working";
  const kind = meeting.minutes?.[0]?.template_id ?? meeting.template_id ?? "";

  useEffect(() => {
    const away = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", away);
    return () => window.removeEventListener("keydown", away);
  }, [onClose]);

  const item =
    "w-full flex items-center justify-between gap-4 h-8 px-3 border-0 rounded-[6px] bg-transparent text-left text-[13.5px] hover:bg-s2";
  return (
    <div
      role="menu"
      className="absolute right-0 top-9 z-20 w-[240px] p-1.5 rounded-[10px] bg-s1 shadow-[0_8px_24px_var(--sh)]"
    >
      <label className="flex items-center justify-between gap-3 h-9 px-3 text-[13.5px]">
        <span className="t2">{t("minutes.template")}</span>
        <select
          className="sel"
          disabled={working || !meeting.transcript}
          value={kind}
          onChange={(event) => {
            void setMeetingInfo(meeting.id, { template_id: event.target.value })
              .then(() => commands.seogiRetry(meeting.id, true))
              .then(() => refreshDetail());
            onClose();
          }}
        >
          {templates.some((item) => item.id === kind) ? null : (
            <option value="">{"–"}</option>
          )}
          {templates.map((item) => (
            <option key={item.id} value={item.id}>
              {item.name}
            </option>
          ))}
        </select>
      </label>
      {meeting.transcript ? (
        <button
          type="button"
          role="menuitem"
          className={item}
          disabled={working}
          onClick={() => {
            void commands
              .seogiRetry(meeting.id, true)
              .then(() => refreshDetail());
            onClose();
          }}
        >
          {t("minutes.rewrite")}
        </button>
      ) : null}
      {meeting.minutes?.length ? (
        <button
          type="button"
          role="menuitem"
          className={item}
          onClick={() => {
            void commands.seogiReveal(meeting.id, "minutes");
            onClose();
          }}
        >
          {t("minutes.showMinutesFile")}
        </button>
      ) : null}
      <button
        type="button"
        role="menuitem"
        className={item}
        onClick={() => {
          void commands.seogiReveal(meeting.id, "recording");
          onClose();
        }}
      >
        {t("minutes.showRecording")}
      </button>
      <button
        type="button"
        role="menuitem"
        className={item}
        onClick={() => {
          void commands.seogiOpenFolder(meeting.id);
          onClose();
        }}
      >
        {t("minutes.openFolder")}
      </button>
      <button type="button" role="menuitem" className={item} onClick={onDelete}>
        <span>{t("trash.menu")}</span>
        {busy(meeting) ? (
          <span className="text-[12px] t3">{t("trash.busyShort")}</span>
        ) : null}
      </button>
    </div>
  );
}

/** The thin line along the bottom that plays the recording: red ticks
 *  where "중요 표시" was pressed, dots where tasks were said. */
function PlayerLine({ player, meeting }: { player: Player; meeting: Meeting }) {
  const { t } = useTranslation(NS);
  const total = Math.max(1, player.durationMs);
  const at = (ms: number) => `${Math.min(100, (ms / total) * 100)}%`;
  const marks = meeting.marks_ms ?? [];
  const said = (meeting.minutes?.[0]?.sections ?? [])
    .filter((section) => section.kind === "actions")
    .flatMap((section) => section.entries)
    .map((entry) => entry.sources[0]?.ms)
    .filter((ms): ms is number => ms !== undefined);
  return (
    <div className="player">
      <button
        type="button"
        className="pl"
        disabled={!player.canPlayThrough}
        aria-label={player.playing ? t("minutes.pause") : t("minutes.listen")}
        onClick={player.toggle}
      >
        {player.playing ? <PauseGlyph /> : <PlayGlyph />}
      </button>
      <span className="tab t2 text-[13.5px]">
        {clock(player.timeMs)}{" "}
        <span className="t3">{`/ ${clock(player.durationMs)}`}</span>
      </span>
      <div
        role="slider"
        tabIndex={0}
        aria-label={t("minutes.position", {
          total: clock(player.durationMs),
          at: clock(player.timeMs),
          marks: marks.length,
        })}
        aria-valuemin={0}
        aria-valuemax={Math.round(total / 1000)}
        aria-valuenow={Math.round(player.timeMs / 1000)}
        className="trk"
        onClick={(event) => {
          const box = event.currentTarget.getBoundingClientRect();
          player.seek(((event.clientX - box.left) / box.width) * total);
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowRight") player.seek(player.timeMs + 5000);
          if (event.key === "ArrowLeft") player.seek(player.timeMs - 5000);
        }}
      >
        <span className="done" style={{ width: at(player.timeMs) }} />
        {marks.map((mark) => (
          <span key={`m${mark}`} className="mk" style={{ left: at(mark) }} />
        ))}
        {said.map((ms, index) => (
          <span key={`e${index}`} className="ev" style={{ left: at(ms) }} />
        ))}
      </div>
      {player.canPlayThrough ? null : (
        <span className="t3 text-[13px]">{t("minutes.cannotPlayShort")}</span>
      )}
      <button
        type="button"
        className="ib"
        disabled={!player.canPlayThrough}
        onClick={player.nextRate}
      >
        {t("minutes.speed", { x: player.rate })}
      </button>
    </div>
  );
}

/** A part of the minutes that opens when pressed. */
function Fold({
  title,
  count,
  countRed = false,
  open = false,
  id,
  children,
}: {
  title: React.ReactNode;
  count?: number;
  countRed?: boolean;
  open?: boolean;
  id?: string;
  children: React.ReactNode;
}) {
  return (
    <details open={open || undefined} id={id}>
      <summary className="fold">
        <span>{title}</span>
        <span className="r">
          {count !== undefined ? (
            <span className={countRed ? "rt" : undefined}>{count}</span>
          ) : null}
          <RightIcon />
        </span>
      </summary>
      <div className="fold-body">{children}</div>
    </details>
  );
}

/** The line of the minutes a search went to, as the sheet lays it out. */
type Spot =
  | { at: "title" | "summary" | "people" }
  | { at: "story"; index: number }
  | { at: "entry"; section: string; index: number }
  | { at: "transcript"; ms: number };

function spotOf(jump: Jump | null, meeting: Meeting): Spot | null {
  if (!jump) return null;
  const place = jump.place;
  if (!place) return has(meeting.title, jump.words) ? { at: "title" } : null;
  const story = meeting.minutes?.[0]?.story ?? [];
  switch (place.kind) {
    case "title":
    case "people":
      return { at: place.kind };
    case "summary":
      // Shown only where there is no story; else the story's top.
      return story.length === 0
        ? { at: "summary" }
        : { at: "story", index: -1 };
    case "story":
      return { at: "story", index: place.index };
    case "entry":
      return { at: "entry", section: place.section, index: place.index };
    case "transcript":
      return place.ms === null ? null : { at: "transcript", ms: place.ms };
  }
}

/** Opens the folds around the line a search went to and brings it to the
 *  middle of the window; the next press anywhere else takes the mark off. */
/** How far in from a scrolling box's edge its scroll bar may be pressed:
 *  a bar that takes room, or one drawn over the content (macOS). */
const SCROLL_BAR = 16;

/** Whether a press is on a scroll bar. A bar that takes room is outside
 *  the box's content, so the press lands on the box itself past its
 *  inner width; one drawn over the content gets the press on whatever is
 *  under it, so any box that scrolls is asked whether the press is near
 *  its edge. */
function onScrollBar(event: PointerEvent): boolean {
  const target = event.target instanceof Element ? event.target : null;
  for (let box = target; box; box = box.parentElement) {
    const style = window.getComputedStyle(box);
    const down =
      box.scrollHeight > box.clientHeight &&
      /auto|scroll/.test(style.overflowY);
    const across =
      box.scrollWidth > box.clientWidth && /auto|scroll/.test(style.overflowX);
    if (!down && !across) continue;
    const edge = box.getBoundingClientRect();
    if (down && edge.right - event.clientX <= SCROLL_BAR) return true;
    if (across && edge.bottom - event.clientY <= SCROLL_BAR) return true;
  }
  return false;
}

function useGoToSpot(
  sheet: React.RefObject<HTMLElement | null>,
  jump: Jump | null,
) {
  const clearJump = useSeogi((s) => s.clearJump);
  useEffect(() => {
    if (!jump) return;
    let second = 0;
    const first = window.requestAnimationFrame(() => {
      const line = sheet.current?.querySelector<HTMLElement>("[data-found]");
      if (!line) return;
      for (
        let fold = line.closest("details");
        fold;
        fold = fold.parentElement?.closest("details") ?? null
      ) {
        fold.open = true;
      }
      const still = window.matchMedia("(prefers-reduced-motion: reduce)");
      // The words themselves, not the top of a long paragraph they are in.
      const words = line.querySelector<HTMLElement>("mark.hl") ?? line;
      second = window.requestAnimationFrame(() =>
        words.scrollIntoView({
          block: "center",
          behavior: still.matches ? "auto" : "smooth",
        }),
      );
    });
    const away = (event: PointerEvent) => {
      // Dragging a scroll bar is scrolling, which keeps the mark.
      if (onScrollBar(event)) return;
      const line = sheet.current?.querySelector("[data-found]");
      if (line && event.target instanceof Node && line.contains(event.target)) {
        return;
      }
      clearJump();
    };
    document.addEventListener("pointerdown", away, true);
    return () => {
      window.cancelAnimationFrame(first);
      window.cancelAnimationFrame(second);
      document.removeEventListener("pointerdown", away, true);
    };
  }, [sheet, jump, clearJump]);
}

function Sheet({
  meeting,
  folder,
  player,
  jump,
}: {
  meeting: Meeting;
  folder: string;
  player: Player;
  jump: Jump | null;
}) {
  const { t, i18n } = useTranslation(NS);
  // The speech model, and the one it stood in for when that was not here.
  const speechBy = (meeting: Meeting) =>
    meeting.transcript
      ? shortModel(meeting.transcript.model) +
        (meeting.stats?.stood_in_for
          ? t("minutes.stoodIn", {
              wanted: shortModel(meeting.stats.stood_in_for),
            })
          : "")
      : "";
  const length = useLength();
  const refreshDetail = useSeogi((s) => s.refreshDetail);
  const sheet = useRef<HTMLElement | null>(null);
  const talk = useTalk(meeting);
  useGoToSpot(sheet, jump);
  const spot = spotOf(jump, meeting);
  const words = jump?.words ?? "";
  const foundEntry = (section: string) =>
    spot?.at === "entry" && spot.section === section ? spot.index : null;
  const minutes = meeting.minutes?.[0];
  const working = meeting.status === "working";
  const listen = (ms: number) => player.seek(ms - 500);
  const hear = (ms: number) => player.seek(ms - 1000, 6000);

  const sections = minutes?.sections ?? [];
  const actionSections = sections.filter((s) => s.kind === "actions");
  const taskCount = actionSections.reduce((n, s) => n + s.entries.length, 0);
  const others = [
    ...sections.filter((s) => s.kind !== "actions" && s.key !== "open"),
    ...sections.filter((s) => s.key === "open"),
  ];
  const story = minutes?.story ?? [];
  const checks = sections.flatMap((section) =>
    section.entries
      .filter((entry) => (entry.reasons ?? []).length > 0)
      .map((entry, index) => ({
        section,
        entry,
        key: `${section.key}-${index}`,
      })),
  );

  const start = meeting.started_at_ms;
  const end = start + meeting.duration_ms;
  const late = lateMinutes(meeting);
  // Only for a consultation: colleagues and memos often start straight in.
  const midTalk =
    (!!meeting.customer?.trim() || meeting.template_id === "consult") &&
    startsMidTalk(meeting.transcript?.segments ?? []);
  const setDone = async (section: string, index: number, done: boolean) => {
    await commands.seogiSetActionDone(meeting.id, section, index, done);
    await refreshDetail();
  };

  return (
    <article ref={sheet}>
      <div className="mb-1 text-[13px] t3">{t("minutes.thisIsMinutes")}</div>
      <h1 className="a-h1" data-found={spot?.at === "title" ? "" : undefined}>
        {spot?.at === "title" ? (
          <Marked text={meeting.title} words={words} />
        ) : (
          meeting.title || t("untitled")
        )}
      </h1>
      <div className="a-meta">
        <span>{`${fullDay(start, i18n.language)} ${hm(start)}`}</span>
        {meeting.duration_ms > 0 ? (
          <span>{length(meeting.duration_ms)}</span>
        ) : null}
      </div>
      {meeting.customer || meeting.place ? (
        <div className="mt-1 text-[13.5px] t2">{headLine(meeting, t)}</div>
      ) : null}
      {late > 0 || midTalk ? (
        <p className="mt-1.5 mb-0 text-[13px] t3">
          {late > 0
            ? t("minutes.startedLate", { n: late })
            : t("minutes.startedMidTalk")}
        </p>
      ) : null}
      <div data-found={spot?.at === "people" ? "" : undefined}>
        <PeopleLine
          meeting={meeting}
          onChanged={refreshDetail}
          className="mt-2"
          mark={spot?.at === "people" ? words : ""}
        />
      </div>
      <TalkShare meeting={meeting} talk={talk} />
      <NameSpeakers meeting={meeting} player={player} talk={talk} />

      {!minutes ? (
        <p className="mt-8 mb-0 max-w-[60ch] text-[15px] leading-[1.7] t2">
          {working ? t("minutes.rewriting") : t("minutes.notReady")}{" "}
          {meeting.error
            ? t(`trouble.${meeting.error}`, { defaultValue: "" })
            : null}
        </p>
      ) : (
        <>
          {taskCount > 0 ? (
            <section className="a-sec" aria-labelledby="tasks-h">
              <h2 id="tasks-h">
                {t("minutes.tasks")}
                <span className="t3">{taskCount}</span>
              </h2>
              {actionSections.map((section) => (
                <TaskGroup
                  key={section.key}
                  section={section}
                  showTitle={titleNeeded(section, actionSections.length)}
                  people={peopleOf(meeting)}
                  found={foundEntry(section.key)}
                  words={words}
                  onListen={listen}
                  onDone={(index, done) =>
                    void setDone(section.key, index, done)
                  }
                />
              ))}
            </section>
          ) : (
            <section className="a-sec" aria-labelledby="tasks-h">
              <h2 id="tasks-h">{t("minutes.tasks")}</h2>
              <p className="m-0 t2">{t("minutes.noTasksPlain")}</p>
            </section>
          )}

          {story.length > 0 || minutes.summary ? (
            <section className="a-sec" aria-labelledby="story-h">
              <h2
                id="story-h"
                data-found={
                  spot?.at === "story" && spot.index < 0 ? "" : undefined
                }
              >
                {t("minutes.story")}
              </h2>
              {story.length > 0 ? (
                story.map((part, index) => {
                  const here = spot?.at === "story" && spot.index === index;
                  return (
                    <div
                      key={`${part.title}-${index}`}
                      className="mb-4"
                      data-found={here ? "" : undefined}
                    >
                      {part.title ? (
                        <h3 className="m-0 mb-0.5 text-[13.5px] font-medium t3">
                          {here ? (
                            <Marked text={part.title} words={words} />
                          ) : (
                            part.title
                          )}
                        </h3>
                      ) : null}
                      <p className="story">
                        {here ? (
                          <Marked text={part.text} words={words} />
                        ) : (
                          part.text
                        )}
                      </p>
                      {part.sources.length > 0 ? (
                        <div className="flex flex-wrap gap-1 mt-1.5">
                          {part.sources.map((source) => (
                            <TimeKey
                              key={source.ms}
                              ms={source.ms}
                              onListen={listen}
                            />
                          ))}
                        </div>
                      ) : null}
                    </div>
                  );
                })
              ) : (
                <p
                  className="story"
                  data-found={spot?.at === "summary" ? "" : undefined}
                >
                  {spot?.at === "summary" ? (
                    <Marked text={minutes.summary} words={words} />
                  ) : (
                    minutes.summary
                  )}
                </p>
              )}
            </section>
          ) : (
            <p className="a-sec mb-0 t3">{t("minutes.nothingToTell")}</p>
          )}
        </>
      )}

      <section className="a-sec" aria-labelledby="more-h">
        <h2 id="more-h">{t("minutes.more")}</h2>
        {minutes?.not_rechecked ? (
          <p className="mt-0 mb-2 text-[13.5px] t3">
            {t("minutes.notRechecked")}
          </p>
        ) : null}
        {checks.length > 0 ? (
          <Fold title={t("minutes.checkFold")} count={checks.length} countRed>
            <p className="mt-0 mb-2 text-[13.5px] t3">
              {t("minutes.checkLead")}
            </p>
            {checks.map(({ section, entry, key }) => (
              <div
                key={key}
                className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 py-2 border-t border-line first:border-t-0"
              >
                <span>
                  <span className="block">{entry.text}</span>
                  <span className="block text-[13px] rt">
                    {t(`minutes.reason.${entry.reasons[0]}`, {
                      defaultValue: t("minutes.reason.weak_support"),
                    })}
                    <span className="t3">{` · ${section.title}`}</span>
                  </span>
                </span>
                {entry.sources[0] ? (
                  <button
                    type="button"
                    className="chip self-start"
                    aria-label={t("minutes.listen5From", {
                      time: clock(entry.sources[0].ms),
                    })}
                    onClick={() => hear(entry.sources[0].ms)}
                  >
                    <PlayGlyph />
                    {t("minutes.listen5")}
                  </button>
                ) : null}
              </div>
            ))}
          </Fold>
        ) : null}
        {others.map((section) => (
          <Fold
            key={section.key}
            title={section.title}
            count={section.entries.length}
          >
            <RuledList
              entries={section.entries}
              found={foundEntry(section.key)}
              words={words}
              onListen={listen}
            />
          </Fold>
        ))}
        {minutes?.feedback ? (
          <Fold title={<b>{t("feedback.title")}</b>}>
            <Feedback meeting={meeting} onListen={listen} onHear={hear} />
          </Fold>
        ) : null}
        <Fold title={t("context.title")}>
          <MeetingContext
            meeting={meeting}
            working={working}
            onChanged={refreshDetail}
            documents={
              <DocumentWords meeting={meeting} onChanged={refreshDetail} />
            }
          />
        </Fold>
        {meeting.transcript ? (
          <Fold title={t("minutes.transcript")} id="transcript-fold">
            <p className="mt-0 mb-2 text-[13px] t3">{t("names.tapToRename")}</p>
            <Transcript
              meeting={meeting}
              player={player}
              found={spot?.at === "transcript" ? spot.ms : null}
              words={words}
            />
          </Fold>
        ) : null}
        {meeting.transcript ? (
          <Fold title={t("minutes.ask")}>
            <Ask id={meeting.id} onListen={listen} />
          </Fold>
        ) : null}
        <Fold title={t("minutes.keptShort")}>
          <p className="mt-0 mb-2 max-w-[66ch] text-[13.5px] t3">
            {t("minutes.keptLead")}
          </p>
          <dl className="m-0 grid grid-cols-[96px_minmax(0,1fr)] gap-x-3 gap-y-1.5 text-[13.5px]">
            <dt className="t3">{t("minutes.file")}</dt>
            <dd className="m-0 break-all">{meeting.audio_file}</dd>
            <dt className="t3">{t("minutes.sha")}</dt>
            <dd className="m-0 break-all tab">{meeting.sha256 || "–"}</dd>
            <dt className="t3">{t("minutes.recordedAt")}</dt>
            <dd className="m-0 tab">
              {t("minutes.recordedRange", {
                day: isoDay(start),
                from: hms(start),
                to: hms(end),
              })}
              {meeting.time_source === "recorder" ||
              meeting.time_source === "file_name" ||
              meeting.time_source === "file_metadata"
                ? null
                : t("minutes.estimated")}
            </dd>
            {meeting.capture_note ? (
              <>
                <dt className="t3">{t("minutes.note")}</dt>
                <dd className="m-0 rt">
                  {t(`minutes.captureNote.${meeting.capture_note}`, {
                    defaultValue: t("minutes.captureNote.file_incomplete"),
                  })}
                </dd>
              </>
            ) : null}
            <dt className="t3">{t("minutes.folder")}</dt>
            <dd className="m-0 break-all">
              {folder}{" "}
              <button
                type="button"
                className="lnk ml-1"
                onClick={() => void commands.seogiOpenFolder(meeting.id)}
              >
                {t("minutes.openFolder")}
              </button>
            </dd>
          </dl>
          <p className="mt-3 mb-0 text-[13px] leading-[1.6] t3">
            {minutes && meeting.transcript
              ? t("minutes.writtenBy", {
                  speech: speechBy(meeting),
                  summary: minutes.written_by,
                })
              : meeting.stats?.stood_in_for
                ? t("minutes.transcribedBy", { speech: speechBy(meeting) })
                : null}
            {(meeting.total_ms ?? 0) > 0
              ? ` · ${t("minutes.took", { length: length(meeting.total_ms ?? 0) })}`
              : null}
            {(meeting.transcript?.corrections ?? []).length > 0
              ? ` · ${t("minutes.corrected")}: ${(
                  meeting.transcript?.corrections ?? []
                )
                  .map((c) => `${c.from} → ${c.to}`)
                  .join(", ")}`
              : null}
          </p>
        </Fold>
      </section>
    </article>
  );
}

function TaskGroup({
  section,
  showTitle,
  people,
  found,
  words,
  onListen,
  onDone,
}: {
  section: MinutesSection;
  showTitle: boolean;
  /** The people of the meeting, whose names a line may leave out. */
  people: string[];
  /** The task a search went to, by its place in the section. */
  found: number | null;
  words: string;
  onListen: (ms: number) => void;
  onDone: (index: number, done: boolean) => void;
}) {
  const { t } = useTranslation(NS);
  const speakerName = useSpeakerName();
  return (
    <div className="mb-2">
      {showTitle ? (
        <h3 className="m-0 mt-2 mb-0.5 text-[13px] font-medium t3">
          {section.title}
        </h3>
      ) : null}
      {section.entries.map((entry, index) => {
        const source = entry.sources[0];
        const { who, text, unowned } = taskLine(section, entry, people);
        const owner = who ? speakerName(who) : "";
        const here = found === index;
        return (
          <div
            key={`${section.key}-${index}`}
            className={`task ${entry.faint ? "opacity-70" : ""}`}
            data-found={here ? "" : undefined}
          >
            <input
              type="checkbox"
              className="chk"
              checked={entry.done ?? false}
              onChange={() => onDone(index, !(entry.done ?? false))}
              aria-label={t("home.doneLabel", { task: entry.text })}
            />
            <span
              className={entry.done ? "t3 line-through" : undefined}
              title={quoteOf(entry) ? `「${quoteOf(entry)}」` : undefined}
            >
              {owner ? (
                <span className="who">
                  {here ? <Marked text={owner} words={words} /> : owner}
                </span>
              ) : null}
              {here ? (
                <Marked
                  text={text}
                  words={words}
                  wholeIfAbsent={!has(owner, words)}
                />
              ) : (
                text
              )}
              {unowned ? (
                <span className="unowned">{t("ownerUnknown")}</span>
              ) : null}
              {(entry.reasons ?? []).length > 0 ? (
                <em>{t("minutes.checkMark")}</em>
              ) : null}
            </span>
            <span className={`due ${entry.due ? "" : "none"}`}>
              {entry.due || t("dueUnknown")}
            </span>
            {source ? <TimeKey ms={source.ms} onListen={onListen} /> : <span />}
          </div>
        );
      })}
    </div>
  );
}

/** The words an entry rests on, unless they only repeat the entry. */
function quoteOf(entry: Entry): string {
  const quote = entry.sources[0]?.quote ?? "";
  return quote && bare(quote) !== bare(entry.text) ? quote : "";
}

function RuledList({
  entries,
  found,
  words,
  onListen,
}: {
  entries: Entry[];
  /** The line a search went to, by its place in the list. */
  found: number | null;
  words: string;
  onListen: (ms: number) => void;
}) {
  const { t } = useTranslation(NS);
  return (
    <ul className="m-0 p-0 list-none">
      {entries.map((entry, index) => {
        const quote = quoteOf(entry);
        const source = entry.sources[0];
        const here = found === index;
        return (
          <li
            key={index}
            data-found={here ? "" : undefined}
            className={`grid grid-cols-[minmax(0,1fr)_auto] gap-3 py-2 border-t border-line first:border-t-0 ${
              entry.faint ? "opacity-70" : ""
            }`}
          >
            <span>
              {here ? (
                <Marked text={entry.text} words={words} wholeIfAbsent />
              ) : (
                entry.text
              )}
              {(entry.reasons ?? []).length > 0 ? (
                <em className="not-italic rt text-[13px] ml-2">
                  {t("minutes.checkMark")}
                </em>
              ) : null}
              {quote ? (
                <span className="block text-[13px] t3">{`「${quote}」`}</span>
              ) : null}
            </span>
            {source ? (
              <TimeKey
                ms={source.ms}
                onListen={onListen}
                className="self-start"
              />
            ) : (
              <span />
            )}
          </li>
        );
      })}
    </ul>
  );
}

interface Asked {
  question: string;
  answer: Answer | null;
  trouble: string;
}

/** Questions put to the recording. Each answer shows the words it rests on;
 *  one whose words are not in the transcript says so. */
function Ask({ id, onListen }: { id: string; onListen: (ms: number) => void }) {
  const { t } = useTranslation(NS);
  const [question, setQuestion] = useState("");
  const [asked, setAsked] = useState<Asked[]>([]);
  const waiting = asked.some((item) => !item.answer && !item.trouble);

  const ask = async () => {
    const words = question.trim();
    if (!words || waiting) return;
    setQuestion("");
    setAsked((list) => [
      { question: words, answer: null, trouble: "" },
      ...list,
    ]);
    const result = await commands.seogiAsk(id, words);
    setAsked((list) =>
      list.map((item, index) =>
        index === 0
          ? result.status === "ok"
            ? { ...item, answer: result.data }
            : { ...item, trouble: result.error }
          : item,
      ),
    );
  };

  return (
    <div>
      <p className="mt-0 mb-2.5 text-[13.5px] text-dim">
        {t("minutes.askLead")}
      </p>
      <form
        className="flex gap-2 pb-3"
        onSubmit={(event) => {
          event.preventDefault();
          void ask();
        }}
      >
        <input
          type="text"
          value={question}
          onChange={(event) => setQuestion(event.target.value)}
          placeholder={t("minutes.askPlaceholder")}
          aria-label={t("minutes.ask")}
          className="field field-soft flex-1 min-w-0"
        />
        <button
          type="submit"
          className="key key-small"
          disabled={waiting || !question.trim()}
        >
          {t("minutes.askButton")}
        </button>
      </form>
      {asked.map((item, index) => (
        <div
          key={asked.length - index}
          className={`py-2.5 ${
            index === 0 ? "border-t border-line" : "border-t border-line"
          }`}
        >
          <strong className="text-[14px]">{item.question}</strong>
          {item.trouble ? (
            <p
              role="alert"
              className="must-check mt-1.5 mb-0 px-2 py-1 font-medium"
            >
              {t("failed", { reason: item.trouble })}
            </p>
          ) : !item.answer ? (
            <p className="mt-1.5 mb-0 text-[13.5px] text-dim">
              {t("minutes.asking")}
            </p>
          ) : (
            <>
              <p
                className={`mt-1.5 mb-0 text-[14px] leading-[1.6] ${
                  item.answer.found ? "" : "text-dim"
                }`}
              >
                {item.answer.text}
              </p>
              {item.answer.unsupported ? (
                <p className="must-check mt-1.5 mb-0 px-2 py-1 text-[13.5px] font-medium">
                  {t("minutes.askUnsupported")}
                </p>
              ) : null}
              {item.answer.sources.map((source) => (
                <div key={source.ms} className="flex items-start gap-2 mt-1.5">
                  <TimeKey
                    ms={source.ms}
                    onListen={onListen}
                    icon={false}
                    className="!h-7 !px-[7px] flex-none"
                  />
                  <span className="pt-0.5 text-[13.5px] text-dim">{`「${source.quote}」`}</span>
                </div>
              ))}
            </>
          )}
        </div>
      ))}
    </div>
  );
}

interface Paragraph {
  startMs: number;
  endMs: number;
  speaker: number;
  text: string;
  unsure: boolean;
}

/** Lines by the same speaker with no real pause between them, as one. */
function paragraphs(segments: Segment[]): Paragraph[] {
  const out: Paragraph[] = [];
  for (const line of segments) {
    const last = out[out.length - 1];
    if (
      last &&
      last.speaker === line.speaker &&
      line.start_ms - last.endMs <= 2000
    ) {
      last.text = `${last.text} ${line.text}`;
      last.endMs = line.end_ms;
      last.unsure = last.unsure || (line.unsure ?? false);
    } else {
      out.push({
        startMs: line.start_ms,
        endMs: line.end_ms,
        speaker: line.speaker,
        text: line.text,
        unsure: line.unsure ?? false,
      });
    }
  }
  return out;
}

/** A speaker who said less than this share is asked about last. */
const QUIET_SHARE = 1;

/** Each speaker's tone (1 … 4), by number, the same in every place a
 *  speaker is shown: a new name keeps it. */
function tonesOf(transcript: MeetingTranscript | null | undefined) {
  const order = (transcript?.segments ?? []).map((line) => line.speaker);
  return speakerTones(
    [...(transcript?.speakers ?? []).map((speaker) => speaker.id), ...order],
    order,
  );
}

/** How the talking went, measured again whenever who said what changes. */
function useTalk(meeting: Meeting): TalkStats | null {
  const [talk, setTalk] = useState<TalkStats | null>(null);
  const shape = (meeting.transcript?.segments ?? [])
    .map((line) => line.speaker)
    .join(",");
  const heard = Boolean(meeting.transcript);
  useEffect(() => {
    if (!heard) return;
    void commands.seogiTalk(meeting.id).then((result) => {
      if (result.status === "ok") setTalk(result.data);
    });
  }, [meeting.id, shape, heard]);
  return heard ? talk : null;
}

/**
 * Who talked how much, as one thin bar split by speaker in their tones,
 * and the names with their share under it; how long each talked shows on
 * pointing at a piece.
 */
function TalkShare({
  meeting,
  talk,
}: {
  meeting: Meeting;
  talk: TalkStats | null;
}) {
  const { t } = useTranslation(NS);
  const length = useLength();
  const speakerName = useSpeakerName();
  const pieces = shareBar(talk?.speakers ?? []);
  if (pieces.length < 2) return null;
  const tones = tonesOf(meeting.transcript);
  const nameOf = (id: number) =>
    speakerName(
      meeting.transcript?.speakers.find((s) => s.id === id)?.name ??
        `화자 ${id}`,
    );
  const percent = (n: number) => (n === 0 ? "<1%" : `${n}%`);
  const hover = (speaker: number, talkMs: number) =>
    t("talkShare.hover", { name: nameOf(speaker), length: length(talkMs) });
  return (
    <div className="mt-3 max-w-[420px]">
      <div
        role="img"
        aria-label={`${t("talkShare.label")}: ${pieces
          .map((p) => `${nameOf(p.speaker)} ${percent(p.percent)}`)
          .join(", ")}`}
        className="flex gap-[2px] h-[3px]"
      >
        {pieces.map((p) => (
          <span
            key={p.speaker}
            title={hover(p.speaker, p.talkMs)}
            className="block rounded-[1.5px] min-w-[2px]"
            style={{
              flexGrow: p.talkMs,
              flexBasis: 0,
              background: `var(--sp${tones.get(p.speaker) ?? 1})`,
            }}
          />
        ))}
      </div>
      <div
        aria-hidden="true"
        className="flex flex-wrap gap-x-3.5 gap-y-0.5 mt-1.5 text-[13px] t2"
      >
        {pieces.map((p) => (
          <span
            key={p.speaker}
            title={hover(p.speaker, p.talkMs)}
            className="inline-flex items-center gap-1"
          >
            <span
              className="inline-flex"
              style={{ color: `var(--sp${tones.get(p.speaker) ?? 1})` }}
            >
              <Mark size={10} tight color="currentColor" className="block" />
            </span>
            {nameOf(p.speaker)}
            <span className="tab t3">{percent(p.percent)}</span>
          </span>
        ))}
      </div>
    </div>
  );
}

/**
 * Naming who spoke, one speaker at a time from a thin line at the top of
 * the minutes (never a window after the recording ends): three lines of
 * each to hear, a name pressed, and on to the next. Opened from the line
 * "N명에 이름이 없어요" for the unnamed, or from "말한 사람 이름 바꾸기"
 * for everyone.
 */
function NameSpeakers({
  meeting,
  player,
  talk,
}: {
  meeting: Meeting;
  player: Player;
  talk: TalkStats | null;
}) {
  const { t } = useTranslation(NS);
  const speakerName = useSpeakerName();
  const refreshDetail = useSeogi((s) => s.refreshDetail);
  // Who is being asked about: the unnamed only, or everyone.
  const [open, setOpen] = useState<false | "unnamed" | "all">(false);
  // Speakers seen this round (named or skipped).
  const [seen, setSeen] = useState<number[]>([]);
  const [other, setOther] = useState<Segment | null>(null);
  if (!meeting.transcript) return null;
  const speakers = meeting.transcript.speakers;
  const share: Record<number, number> = {};
  for (const s of talk?.speakers ?? []) share[s.speaker] = s.share;

  // Those who said most first, the quiet ones last.
  const talked = speakers
    .filter((s) => (share[s.id] ?? 0) > 0)
    .sort((a, b) => (share[b.id] ?? 0) - (share[a.id] ?? 0));
  const unnamed = talked.filter((s) => isUnnamed(s.name));
  const loud = (list: typeof talked) =>
    list.filter((s) => (share[s.id] ?? 0) >= QUIET_SHARE);
  const quiet = (list: typeof talked) =>
    list.filter((s) => (share[s.id] ?? 0) < QUIET_SHARE);
  const asked = open === "all" ? talked : unnamed;
  const queue = [...loud(asked), ...quiet(asked)].filter(
    (s) => !seen.includes(s.id),
  );
  const current = open ? queue[0] : undefined;

  const begin = (who: "unnamed" | "all") => {
    setSeen([]);
    setOther(null);
    setOpen(who);
  };
  // On to the next speaker; after the last, done.
  const next = () => {
    if (!current) return;
    setOther(null);
    setSeen((now) => [...now, current.id]);
    if (queue.length <= 1) setOpen(false);
  };
  const give = async (name: string, line: Segment | null) => {
    if (!current) return;
    await commands.seogiNameSpeaker(
      meeting.id,
      current.id,
      name,
      line ? "here" : "all",
      line ? line.start_ms : 0,
      line ? line.end_ms : 0,
    );
    // One line given to someone else: the same speaker is still asked.
    if (line) setOther(null);
    else next();
    await refreshDetail();
  };

  if (!current) {
    return (
      <>
        {talked.length > 0 ? (
          <button
            type="button"
            className="lnk mt-1.5 text-[13.5px]"
            onClick={() => begin("all")}
          >
            {t("names.renameSpeakers")}
          </button>
        ) : null}
        {unnamed.length > 0 ? (
          <div className="flex flex-wrap items-center gap-x-3 mt-3 py-2 px-3 rounded-[9px] bg-s1 text-[13.5px]">
            <span>
              {t("names.unnamed", {
                n: loud(unnamed).length || unnamed.length,
              })}
            </span>
            <button
              type="button"
              className="lnk"
              onClick={() => begin("unnamed")}
            >
              {t("names.start")}
            </button>
          </div>
        ) : null}
      </>
    );
  }
  const lines = sampleLines(
    meeting.transcript.segments.filter((line) => line.speaker === current.id),
    meeting.duration_ms,
  );
  const quietOne =
    isUnnamed(current.name) && (share[current.id] ?? 0) < QUIET_SHARE;
  // One person the speaker model may have heard as two: offered, never done.
  const sameId = maybeSamePerson(
    current.id,
    speakers,
    meeting.transcript.segments.map((line) => line.speaker),
    isUnnamed,
  );
  const same = speakers.find((s) => s.id === sameId);
  const noPeople = (meeting.people ?? []).length === 0;
  return (
    <section
      aria-label={t("names.cardLabel")}
      className="mt-3 p-4 rounded-[10px] bg-s1 text-[14px]"
    >
      {noPeople ? (
        <div className="mb-3 pb-3 border-b border-line">
          <div className="mb-2 font-medium">{t("names.peopleFirst")}</div>
          <PeopleLine meeting={meeting} onChanged={refreshDetail} open />
        </div>
      ) : null}
      <div className="flex flex-wrap items-baseline gap-x-3 mb-1">
        <b className="text-[15px]">
          {quietOne ? t("names.quiet") : speakerName(current.name)}
        </b>
        <span className="text-[13px] t3">
          {t("names.cardShare", {
            share: Math.round(share[current.id] ?? 0),
            left: queue.length,
          })}
        </span>
        <span className="ml-auto flex gap-3">
          <button type="button" className="lnk text-[13px]" onClick={next}>
            {t("names.skip")}
          </button>
          <button
            type="button"
            className="lnk text-[13px]"
            onClick={() => setOpen(false)}
          >
            {t("names.later")}
          </button>
        </span>
      </div>
      <div className="mb-2 text-[13px] t3">{t("names.hearThree")}</div>
      <ul className="m-0 mb-3 p-0 list-none">
        {lines.map((line) => (
          <li key={line.start_ms} className="py-1.5 border-t border-line">
            <div className="flex items-start gap-2.5">
              <TimeKey
                ms={line.start_ms}
                onListen={() =>
                  player.seek(line.start_ms, line.end_ms - line.start_ms + 500)
                }
              />
              <span className="flex-1">{`「${line.text}」`}</span>
              <button
                type="button"
                className="lnk text-[13px] t3"
                aria-expanded={other === line}
                onClick={() => setOther(other === line ? null : line)}
              >
                {t("names.someoneElse")}
              </button>
            </div>
            {other === line ? (
              <div className="mt-2 ml-[62px]">
                <div className="mb-1 text-[13px] t3">
                  {t("names.whoSaidLine")}
                </div>
                <NamePicker
                  meeting={meeting}
                  current={current.name}
                  onPick={(name) => void give(name, line)}
                />
              </div>
            ) : null}
          </li>
        ))}
      </ul>
      {same ? (
        <div className="mb-3 text-[13.5px]">
          <span className="t2">
            {t("names.samePerson", { name: same.name })}
          </span>{" "}
          <button
            type="button"
            className="lnk"
            onClick={() => void give(same.name, null)}
          >
            {t("names.merge", { name: same.name })}
          </button>
        </div>
      ) : null}
      <div className="mb-1.5 font-medium">{t("names.whoSaid")}</div>
      <NamePicker
        meeting={meeting}
        current={current.name}
        onPick={(name) => void give(name, null)}
      />
      <div className="mt-2 text-[13px] t3">{t("names.followHint")}</div>
    </section>
  );
}

/**
 * Who a speaker is, from a line in the transcript: a name pressed is given
 * at once, to the whole meeting unless "from here" or "this line" is chosen.
 */
function SpeakerNamer({
  meeting,
  speaker,
  current,
  fromMs,
  toMs,
  onDone,
}: {
  meeting: Meeting;
  speaker: number;
  current: string;
  fromMs: number;
  toMs: number;
  onDone: (changed: boolean) => Promise<void> | void;
}) {
  const { t } = useTranslation(NS);
  const [scope, setScope] = useState<NameScope>("all");
  const [trouble, setTrouble] = useState("");

  const give = async (name: string) => {
    const result = await commands.seogiNameSpeaker(
      meeting.id,
      speaker,
      name,
      scope,
      fromMs,
      toMs,
    );
    if (result.status === "error") {
      setTrouble(t("failed", { reason: result.error }));
      return;
    }
    await onDone(true);
  };

  return (
    <div
      role="group"
      aria-label={t("names.title")}
      className="mt-2 mb-1 p-3 rounded-[10px] bg-s1 text-[13.5px]"
    >
      <NamePicker
        meeting={meeting}
        current={current}
        onPick={(name) => void give(name)}
      />
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 mt-2 text-[13px] t3">
        <span>{t("names.where")}</span>
        {(["all", "from_here", "here"] as const).map((option) => (
          <button
            key={option}
            type="button"
            aria-pressed={scope === option}
            onClick={() => setScope(option)}
            className={`p-0 border-0 bg-transparent ${
              scope === option ? "t1 underline underline-offset-4" : "t3"
            }`}
          >
            {t(`names.scope.${option}`)}
          </button>
        ))}
        <button
          type="button"
          className="lnk ml-auto text-[13px]"
          onClick={() => void onDone(false)}
        >
          {t("names.cancel")}
        </button>
      </div>
      {trouble ? <p className="m-0 mt-1.5 rt">{trouble}</p> : null}
    </div>
  );
}

function Transcript({
  meeting,
  player,
  found,
  words,
}: {
  meeting: Meeting;
  player: Player;
  /** When the line a search went to starts. */
  found: number | null;
  words: string;
}) {
  const { t } = useTranslation(NS);
  const speakerName = useSpeakerName();
  const refreshDetail = useSeogi((s) => s.refreshDetail);
  // The line (by its start) whose speaker is being named.
  const [naming, setNaming] = useState<number | null>(null);
  const current = useRef<HTMLDivElement | null>(null);
  const transcript = meeting.transcript;
  const marks = meeting.marks_ms ?? [];

  const lines = useMemo(
    () => paragraphs(transcript?.segments ?? []),
    [transcript],
  );
  // Each speaker's tone, by number: a new name keeps it.
  const tones = useMemo(() => tonesOf(transcript), [transcript]);
  // A line the speech model made up is shown faintly; the minutes did not
  // rest on it. Stray foreign letters count only in a Korean meeting.
  const korean = useMemo(
    () => koreanMeeting(transcript?.segments ?? []),
    [transcript],
  );
  const nowIndex = lines.findIndex(
    (line, index) =>
      player.timeMs >= line.startMs &&
      player.timeMs < (lines[index + 1]?.startMs ?? Number.MAX_SAFE_INTEGER),
  );
  const following = player.playing || player.timeMs > 0;
  // Lines said together are shown as one: the one the found line is in.
  const foundIndex =
    found === null
      ? -1
      : lines.reduce(
          (last, line, index) => (line.startMs <= found ? index : last),
          -1,
        );

  useEffect(() => {
    if (player.playing) {
      current.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }
  }, [nowIndex, player.playing]);

  if (!transcript || lines.length === 0) return null;
  const nameOf = (id: number) =>
    transcript.speakers.find((speaker) => speaker.id === id)?.name ?? "";

  return (
    <div
      aria-label={t("minutes.transcript")}
      className="max-h-[70vh] overflow-y-auto pr-1"
    >
      <p className="mt-0 mb-2 text-[13.5px] t3">
        {t("minutes.transcriptLead")}
      </p>
      {lines.map((line, index) => {
        const on = following && index === nowIndex;
        const marked = marks.some(
          (mark) =>
            mark >= line.startMs - 2000 &&
            mark < (lines[index + 1]?.startMs ?? line.endMs + 1),
        );
        const raw = nameOf(line.speaker);
        const band = on || marked;
        // The same speaker going on after a pause: the mark is left out,
        // the name kept quiet (pressing it still names this line).
        const goingOn = index > 0 && lines[index - 1].speaker === line.speaker;
        const noise = isGibberish(line.text, korean);
        return (
          <div
            key={line.startMs}
            ref={on ? current : undefined}
            data-found={index === foundIndex ? "" : undefined}
            className={`py-2 px-2 -mx-2 rounded-[8px] ${
              band
                ? marked
                  ? "bg-s1 shadow-[inset_2px_0_0_var(--red)]"
                  : "bg-s1"
                : ""
            }`}
          >
            <div className="flex items-center gap-2.5">
              <TimeKey
                ms={line.startMs}
                onListen={(ms) => player.seek(ms)}
                active={on}
                icon={false}
              />
              <span
                className="inline-flex shrink-0 w-3.5 -mr-1"
                style={{ color: `var(--sp${tones.get(line.speaker) ?? 1})` }}
              >
                {goingOn ? null : (
                  <Mark
                    size={14}
                    tight
                    color="currentColor"
                    className="block"
                  />
                )}
              </span>
              <button
                type="button"
                aria-expanded={naming === line.startMs}
                aria-label={t("names.change", { name: speakerName(raw) })}
                onClick={() =>
                  setNaming(naming === line.startMs ? null : line.startMs)
                }
                className={`p-0 border-0 bg-transparent text-[13.5px] font-medium underline decoration-dotted decoration-[var(--t3)] underline-offset-4 cursor-pointer ${
                  goingOn ? "t3" : ""
                }`}
              >
                {speakerName(raw)}
              </button>
              {on || marked ? (
                <span className="ml-auto text-[13px] rt">
                  {[
                    marked ? t("minutes.markedHere") : "",
                    on ? t("minutes.nowHere") : "",
                  ]
                    .filter(Boolean)
                    .join(", ")}
                </span>
              ) : null}
            </div>
            {naming === line.startMs ? (
              <SpeakerNamer
                meeting={meeting}
                speaker={line.speaker}
                current={raw}
                fromMs={line.startMs}
                toMs={line.endMs}
                onDone={async (changed) => {
                  setNaming(null);
                  if (changed) await refreshDetail();
                }}
              />
            ) : null}
            <p
              title={noise ? t("minutes.noiseLine") : undefined}
              className={`mt-1 mb-0 text-[14.5px] leading-[1.7] ${
                on ? "font-medium" : ""
              } ${noise ? "t3 opacity-60" : ""} ${line.unsure ? "underline decoration-dotted decoration-[var(--t3)] underline-offset-4" : ""}`}
            >
              {index === foundIndex ? (
                <Marked text={line.text} words={words} />
              ) : (
                line.text
              )}
            </p>
          </div>
        );
      })}
    </div>
  );
}
