// The minutes laid out in one column: tasks, the story, then "자세히".

import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { commands, type Meeting } from "@/bindings";
import { TimeKey, useLength } from "../../components/common";
import DocumentWords from "../../components/DocumentWords";
import { Marked } from "../../components/Found";
import Feedback from "../../components/Feedback";
import { PlayGlyph } from "../../components/icons";
import MeetingContext from "../../components/MeetingContext";
import { PeopleLine } from "../../components/People";
import { useConfirmRedo } from "../../components/Edit";
import { NS } from "../../i18n";
import { clock, fullDay, hm, hms, isoDay, shortModel } from "../../lib/format";
import { type Player } from "../../lib/player";
import { titleNeeded } from "../../lib/tasks";
import { lateMinutes, startsMidTalk } from "../../lib/opening";
import { OtherEdits } from "../../components/OtherEdits";
import { useSeogi, type Jump } from "../../store";
import { Ask, RuledList } from "./Details";
import {
  Fold,
  headLine,
  peopleOf,
  spotOf,
  useGapText,
  useGoToSpot,
} from "./Parts";
import { NameSpeakers, TalkShare, useTalk } from "./Speakers";
import { NoTasksAdd, TaskGroup } from "./Tasks";
import { Transcript } from "./Transcript";

export function Sheet({
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
  const gapText = useGapText();
  // A stand-in transcribed this meeting, and the model it stood in for
  // is here now: offer to transcribe it again with that one.
  const readiness = useSeogi((s) => s.readiness);
  const wanted = meeting.stats?.stood_in_for ?? "";
  const redoWith =
    wanted &&
    meeting.status !== "working" &&
    meeting.status !== "queued" &&
    readiness?.speech_models.some(
      (option) => option.id === wanted && option.downloaded,
    )
      ? wanted
      : null;
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
  const redoSpeech = useConfirmRedo(meeting);
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

  // What needs a look sits folded under 자세히; a red line by 할 일 says
  // so at the top and opens it, so it is not found only by scrolling down.
  const goCheck = () => {
    const fold = document.getElementById("check-fold");
    if (!(fold instanceof HTMLDetailsElement)) return;
    fold.open = true;
    const still = window.matchMedia("(prefers-reduced-motion: reduce)");
    fold.scrollIntoView({
      block: "start",
      behavior: still.matches ? "auto" : "smooth",
    });
    // The keyboard goes there too, so the next Tab is in the list.
    fold.querySelector("summary")?.focus({ preventScroll: true });
  };
  const checkLink =
    checks.length > 0 ? (
      <button type="button" className="lnk ml-auto rt" onClick={goCheck}>
        {t("minutes.toCheck", { n: checks.length })}
      </button>
    ) : null;

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
          <OtherEdits meeting={meeting} onChanged={refreshDetail} />
          {taskCount > 0 ||
          actionSections.some((s) => (s.removed ?? []).length > 0) ? (
            <section className="a-sec" aria-labelledby="tasks-h">
              <h2 id="tasks-h">
                {t("minutes.tasks")}
                <span className="t3">{taskCount}</span>
                {checkLink}
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
                  meetingId={meeting.id}
                  onChanged={refreshDetail}
                />
              ))}
            </section>
          ) : (
            <section className="a-sec" aria-labelledby="tasks-h">
              <h2 id="tasks-h">
                {t("minutes.tasks")}
                {checkLink}
              </h2>
              <p className="m-0 t2">{t("minutes.noTasksPlain")}</p>
              <NoTasksAdd
                meetingId={meeting.id}
                section={actionSections[0]?.key ?? ""}
                onChanged={refreshDetail}
              />
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
                        <h3 className="m-0 mb-0.5 text-[13.5px] font-normal t3">
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
          <Fold
            id="check-fold"
            title={t("minutes.checkFold")}
            count={checks.length}
            countRed
          >
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
            {meeting.gaps && meeting.gaps.length > 0 ? (
              <>
                <dt className="t3">{t("minutes.gapsLabel")}</dt>
                <dd className="m-0">
                  {[...meeting.gaps]
                    .sort((a, b) => a.at_ms - b.at_ms)
                    .map((gap) => `${clock(gap.at_ms)} ${gapText(gap)}`)
                    .join(", ")}
                  <span className="t3"> {t("minutes.gapsLead")}</span>
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
            {redoWith ? (
              <>
                {" "}
                <button
                  type="button"
                  className="lnk"
                  onClick={() =>
                    redoSpeech.ask("speech", () => {
                      void commands
                        .seogiRetranscribe(meeting.id)
                        .then(() => refreshDetail());
                    })
                  }
                >
                  {t("minutes.redoWith", { model: shortModel(redoWith) })}
                </button>
              </>
            ) : null}
          </p>
        </Fold>
      </section>
      {redoSpeech.dialog}
    </article>
  );
}
