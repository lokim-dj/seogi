// Pieces the minutes sheet shares: the head line, the playing line, folds,
// and going to the line a search found.

import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { type Gap, type Meeting } from "@/bindings";
import { has } from "../../lib/found";
import { PauseGlyph, PlayGlyph, RightIcon } from "../../components/icons";
import { NS } from "../../i18n";
import { clock } from "../../lib/format";
import { type Player } from "../../lib/player";
import { useSeogi, type Jump } from "../../store";

export const bare = (text: string) => text.replace(/[\s.,!?'"「」…·]/g, "");

/** Whom and where the meeting is about, in one line: "고객 이서진 · 장소
 *  한빛아파트 103동". Empty when neither is known. */
export function headLine(meeting: Meeting, t: (key: string) => string): string {
  const parts: string[] = [];
  if (meeting.customer)
    parts.push(`${t("minutes.customer")} ${meeting.customer}`);
  if (meeting.place) parts.push(`${t("minutes.place")} ${meeting.place}`);
  return parts.join(" · ");
}

/** The people of a meeting, whose names a task line may leave out. */
export const peopleOf = (meeting: Meeting) => [
  ...(meeting.people ?? []),
  ...(meeting.transcript?.speakers ?? []).map((speaker) => speaker.name),
];

/** The thin line along the bottom that plays the recording: red ticks
 *  where "중요 표시" was pressed, dots where tasks were said. */
export function PlayerLine({
  player,
  meeting,
}: {
  player: Player;
  meeting: Meeting;
}) {
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
export function Fold({
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

export function spotOf(jump: Jump | null, meeting: Meeting): Spot | null {
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

/** How long the microphone was away, as the transcript says it: (12초 끊김). */
export function useGapText() {
  const { t } = useTranslation(NS);
  return (gap: Gap) => {
    // The app stopped while the microphone was away: no length is known.
    if (gap.gap_ms === 0) return t("minutes.gapOpen");
    const seconds = Math.max(1, Math.ceil(gap.gap_ms / 1000));
    const h = Math.floor(seconds / 3600);
    const m = Math.floor(seconds / 60) % 60;
    const s = seconds % 60;
    const parts: string[] = [];
    if (h > 0) parts.push(t("minutes.gapLength.h", { n: h }));
    if (m > 0) parts.push(t("minutes.gapLength.m", { n: m }));
    if (s > 0 && h === 0) parts.push(t("minutes.gapLength.s", { n: s }));
    return t("minutes.gap", { length: parts.join(" ") });
  };
}

export function useGoToSpot(
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
