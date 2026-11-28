// A meeting's minutes, read in one column: who does what, how the meeting
// went, and everything else folded under "자세히". Every line points at the
// moment of the recording it rests on; a thin line at the bottom plays it.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type Meeting, type Template } from "@/bindings";
import { BackIcon, CopyIcon, MoreIcon } from "../components/icons";
import { setMeetingInfo } from "../components/MeetingContext";
import { ConfirmDelete, busy } from "../components/DeleteMeeting";
import { useConfirmRedo, type Redo } from "../components/Edit";
import { SummaryCardDialog } from "../components/SummaryCard";
import { NS } from "../i18n";
import { clock } from "../lib/format";
import { joinPath, usePlayer } from "../lib/player";
import { taskLine, titleNeeded } from "../lib/tasks";
import { useSeogi } from "../store";
import { headLine, peopleOf, PlayerLine } from "./minutes/Parts";
import { Sheet } from "./minutes/Sheet";

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
  // Asks before writing again would undo the user's changes; it outlives
  // the menu it is asked from.
  const redo = useConfirmRedo(meeting);

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
              onRedo={redo.ask}
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
          {redo.dialog}
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
  onRedo,
  onClose,
  onDelete,
}: {
  meeting: Meeting;
  templates: Template[];
  /** Goes on with writing again, asking first when it would undo changes. */
  onRedo: (redo: Redo, go: () => void) => void;
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
            const template = event.target.value;
            onRedo("minutes", () => {
              void setMeetingInfo(meeting.id, { template_id: template })
                .then(() => commands.seogiRetry(meeting.id, true))
                .then(() => refreshDetail());
            });
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
            onRedo("minutes", () => {
              void commands
                .seogiRetry(meeting.id, true)
                .then(() => refreshDetail());
            });
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
