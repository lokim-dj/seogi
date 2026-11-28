// Who spoke: how the talking was shared, and naming the speakers.

import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type Meeting,
  type NameScope,
  type Naming,
  type TalkStats,
  type Transcript as MeetingTranscript,
} from "@/bindings";
import {
  isUnnamed,
  TimeKey,
  useLength,
  useSpeakerName,
} from "../../components/common";
import { NamePicker, PeopleLine } from "../../components/People";
import { pieces } from "../../lib/wordfix";
import { NS } from "../../i18n";
import { clock } from "../../lib/format";
import { type Player } from "../../lib/player";
import { maybeSamePerson, shareBar, speakerTones } from "../../lib/speakers";
import {
  answer,
  ASK_LINES,
  askLines,
  askMore,
  joinsWith,
  keptNaming,
  linesLeft,
  named,
  nameKey,
  namingSteps,
  QUIET_SHARE,
  quotedWithWa,
  SAME_ENOUGH,
  sameNaming,
  speakerQueue,
  startAsking,
  verdict,
  withIs,
  withRo,
  type NameStep,
} from "../../lib/naming";
import { Mark } from "../../components/Mark";
import { useSeogi } from "../../store";

/** Each speaker's tone (1 … 4), by number, the same in every place a
 *  speaker is shown: a new name keeps it. */
export function tonesOf(transcript: MeetingTranscript | null | undefined) {
  const order = (transcript?.segments ?? []).map((line) => line.speaker);
  return speakerTones(
    [...(transcript?.speakers ?? []).map((speaker) => speaker.id), ...order],
    order,
  );
}

/** How the talking went, measured again whenever who said what changes. */
export function useTalk(meeting: Meeting): TalkStats | null {
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
export function TalkShare({
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
 * the minutes (never a window after the recording ends): up to five of the
 * speaker's lines to hear, one at a time, each given a name or "모르겠어요";
 * when four agree the whole speaker becomes that person on a press, and the
 * lines given someone else go to that one. What was just done can be put
 * back once. Opened from the line "N명에 이름이 없어요" for the unnamed, or
 * from "말한 사람 이름 바꾸기" for everyone.
 */
export function NameSpeakers({
  meeting,
  player,
  talk,
}: {
  meeting: Meeting;
  player: Player;
  talk: TalkStats | null;
}) {
  const { t } = useTranslation(NS);
  const refreshDetail = useSeogi((s) => s.refreshDetail);
  // Who is being asked about: the unnamed only, or everyone.
  const [open, setOpen] = useState<false | "unnamed" | "all">(false);
  // Speakers seen this round (named or skipped), and those there when it
  // began (a speaker made by moving a line is not asked about).
  const [seen, setSeen] = useState<number[]>([]);
  const [from, setFrom] = useState<number[] | null>(null);
  // Names typed this round, offered as keys from then on.
  const [typed, setTyped] = useState<string[]>([]);
  // The naming just made, to put back once: as it was before (`kept`) and
  // just after (`now`). It goes when anything about who said what changes
  // since, or the person moves on.
  const [undo, setUndo] = useState<{
    kept: Naming;
    now: Naming;
    said: string;
    speaker: number;
    who: "unnamed" | "all";
  } | null>(null);
  const [undoing, setUndoing] = useState(false);
  const [said, setSaid] = useState("");
  const naming = useMemo(
    () =>
      meeting.transcript
        ? keptNaming(meeting.transcript, meeting.minutes ?? [])
        : null,
    [meeting.transcript, meeting.minutes],
  );
  if (!meeting.transcript) return null;
  const canUndo = Boolean(undo && naming && sameNaming(undo.now, naming));
  const speakers = meeting.transcript.speakers;
  const share: Record<number, number> = {};
  for (const s of talk?.speakers ?? []) share[s.speaker] = s.share;

  const everyone = speakerQueue(speakers, share, "all", isUnnamed, [], null);
  const unnamed = speakerQueue(speakers, share, "unnamed", isUnnamed, [], null);
  const queue = open
    ? speakerQueue(speakers, share, open, isUnnamed, seen, from)
    : [];
  const current = queue[0];

  const begin = (who: "unnamed" | "all") => {
    setSeen([]);
    setFrom(speakers.map((s) => s.id));
    setTyped([]);
    setUndo(null);
    setSaid("");
    setOpen(who);
  };
  // On to the next speaker; after the last, done.
  const next = () => {
    if (!current) return;
    setSeen((now) => [...now, current.id]);
    if (queue.length <= 1) setOpen(false);
  };
  // A naming made of speaker `id`; after a part of it failed and it was
  // tried again, what was there before the first try is what goes back.
  const settled = (
    id: number,
    kept: Naming,
    now: Naming | null,
    text: string,
    done: boolean,
  ) => {
    if (!open) return;
    setUndo((was) => {
      if (!now) return null;
      if (was && was.speaker === id) return { ...was, now, said: text };
      return { kept, now, said: text, speaker: id, who: open };
    });
    setSaid("");
    if (done) {
      setSeen((seenNow) => [...seenNow, id]);
      if (queue.length <= 1) setOpen(false);
    }
  };
  const putBack = async () => {
    if (!undo || !canUndo) return;
    setUndoing(true);
    const result = await commands.seogiRestoreNaming(
      meeting.id,
      undo.kept,
      undo.now,
    );
    setUndoing(false);
    if (result.status === "error") {
      setSaid(t("failed", { reason: result.error }));
      return;
    }
    // The speaker is asked about again.
    setSeen((now) => now.filter((id) => id !== undo.speaker));
    setOpen(undo.who);
    setUndo(null);
    setSaid(t("names.undone"));
    await refreshDetail();
  };

  const undoLine =
    canUndo || said ? (
      <div
        className="flex flex-wrap items-center gap-x-3 mt-3 text-[13.5px]"
        aria-live="polite"
      >
        {undo && canUndo ? <span className="t2">{undo.said}</span> : null}
        {canUndo ? (
          <button
            type="button"
            className="lnk"
            disabled={undoing}
            onClick={() => void putBack()}
          >
            {t("names.undo")}
          </button>
        ) : null}
        {said ? (
          <span role={canUndo ? undefined : "status"} className="t2">
            {said}
          </span>
        ) : null}
      </div>
    ) : null;

  if (!current) {
    const loud = unnamed.filter((s) => (share[s.id] ?? 0) >= QUIET_SHARE);
    return (
      <>
        {undoLine}
        {everyone.length > 0 ? (
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
              {t("names.unnamed", { n: loud.length || unnamed.length })}
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
  return (
    <>
      {undoLine}
      <AskSpeaker
        key={current.id}
        meeting={meeting}
        player={player}
        speaker={current}
        share={share[current.id] ?? 0}
        left={queue.length}
        typed={typed}
        onTyped={(name) => setTyped((now) => [...now, name])}
        onSettled={settled}
        onNext={() => {
          setUndo(null);
          setSaid("");
          next();
        }}
        onLater={() => {
          setUndo(null);
          setSaid("");
          setOpen(false);
        }}
      />
    </>
  );
}

/** The card for one speaker: lines to hear, a name for each, then settle. */
function AskSpeaker({
  meeting,
  player,
  speaker,
  share,
  left,
  typed,
  onTyped,
  onSettled,
  onNext,
  onLater,
}: {
  meeting: Meeting;
  player: Player;
  speaker: { id: number; name: string };
  share: number;
  left: number;
  /** Names typed earlier this round. */
  typed: string[];
  onTyped: (name: string) => void;
  /** A naming of the speaker was made (`done`: all of it, on to the next
   *  speaker): who said what before it and just after, and what to say. */
  onSettled: (
    id: number,
    kept: Naming,
    now: Naming | null,
    said: string,
    done: boolean,
  ) => void;
  onNext: () => void;
  onLater: () => void;
}) {
  const { t } = useTranslation(NS);
  const speakerName = useSpeakerName();
  const refreshDetail = useSeogi((s) => s.refreshDetail);
  const transcript = meeting.transcript;
  // The names already written down: a name given as one of these, "님" or
  // not, is written the same way.
  const known = [
    ...(meeting.people ?? []),
    ...(transcript?.speakers ?? [])
      .map((s) => s.name)
      .filter((name) => !isUnnamed(name)),
    ...typed,
  ];
  const [state, setState] = useState(() =>
    startAsking(
      askLines(
        // In the user's words where they fixed a line.
        (transcript?.segments ?? [])
          .map((line) => ({ ...line, text: line.edited ?? line.text }))
          .filter((line) => line.speaker === speaker.id && line.text.trim()),
        meeting.duration_ms,
      ),
      isUnnamed(speaker.name) ? "" : speaker.name,
      known,
    ),
  );
  const [working, setWorking] = useState(false);
  const [trouble, setTrouble] = useState("");
  const settleKey = useRef<HTMLButtonElement>(null);
  const v = verdict(state);
  // When the answers say something, the key to act on it takes the focus.
  useEffect(() => {
    if (v.kind !== "asking") settleKey.current?.focus();
  }, [v.kind]);
  if (!transcript) return null;

  const tone = tonesOf(transcript).get(speaker.id) ?? 1;
  const shown = speakerName(speaker.name);
  const quietOne = isUnnamed(speaker.name) && share < QUIET_SHARE;
  const count = named(state);
  const line = v.kind === "asking" ? state.lines[state.answers.length] : null;
  const canMore = linesLeft(state) > 0 && count < ASK_LINES;
  const listen = (at: { start_ms: number; end_ms: number }) =>
    player.seek(at.start_ms, at.end_ms - at.start_ms + 500);
  // Typed names offered as keys: those the people list does not have.
  const people = meeting.people ?? [];
  const more = typed.filter(
    (name) => !people.some((p) => nameKey(p) === nameKey(name)),
  );
  const pick = (name: string | null) => {
    if (working) return;
    if (name && !known.some((k) => nameKey(k) === nameKey(name))) {
      onTyped(name.trim());
    }
    setState((now) => answer(now, name));
  };
  const joins =
    v.kind === "decide"
      ? joinsWith(speaker.id, v.name, transcript.speakers)
      : null;

  // Gives the names, single lines first, keeping what was there before so it
  // can be put back; a failure stops there and says so.
  // Who said what as the backend has it now, read again after a naming.
  const after = () => {
    const fresh = useSeogi.getState().detail;
    return fresh?.id === meeting.id && fresh.transcript
      ? keptNaming(fresh.transcript, fresh.minutes ?? [])
      : null;
  };
  const settle = async (steps: NameStep[], said: string) => {
    const naming = keptNaming(transcript, meeting.minutes ?? []);
    setWorking(true);
    setTrouble("");
    let done = 0;
    for (const step of steps) {
      const result = await commands.seogiNameSpeaker(
        meeting.id,
        speaker.id,
        step.name,
        step.line ? "here" : "all",
        step.line?.start_ms ?? 0,
        step.line?.end_ms ?? 0,
      );
      if (result.status === "error") {
        setTrouble(t("failed", { reason: result.error }));
        setWorking(false);
        await refreshDetail();
        if (done > 0) {
          onSettled(speaker.id, naming, after(), t("names.partial"), false);
        }
        return;
      }
      done += 1;
    }
    await refreshDetail();
    setWorking(false);
    onSettled(speaker.id, naming, after(), said, true);
  };
  const settleSaid =
    v.kind === "decide"
      ? t("names.settled", {
          speaker: shown,
          name: v.name,
          nameRo: withRo(v.name),
        })
      : v.kind === "mixed"
        ? t("names.moved", { n: v.move.length })
        : "";
  const headline =
    v.kind === "decide"
      ? t("names.agreed", {
          of: v.of,
          agree: v.agree,
          name: v.name,
          nameIs: withIs(v.name),
        })
      : v.kind === "mixed"
        ? t("names.mixed", { n: v.move.length })
        : v.kind === "none"
          ? t("names.unknown")
          : "";

  // One person the speaker model may have heard as two: offered, never done.
  const sameId = maybeSamePerson(
    speaker.id,
    transcript.speakers,
    transcript.segments.map((segment) => segment.speaker),
    isUnnamed,
  );
  const same = transcript.speakers.find((s) => s.id === sameId);
  const noPeople = people.length === 0;

  return (
    <section
      aria-label={t("names.cardLabel")}
      className="mt-3 p-4 rounded-[10px] bg-s1 text-[14px]"
    >
      {noPeople ? (
        <div className="mb-3 pb-3 border-b border-line">
          <div className="mb-2 font-normal">{t("names.peopleFirst")}</div>
          <PeopleLine meeting={meeting} onChanged={refreshDetail} open />
        </div>
      ) : null}
      <div className="flex flex-wrap items-baseline gap-x-2.5 mb-0.5">
        <b className="inline-flex items-center gap-1 text-[15px]">
          <span className="inline-flex" style={{ color: `var(--sp${tone})` }}>
            <Mark size={10} tight color="currentColor" className="block" />
          </span>
          {quietOne ? t("names.quiet") : shown}
        </b>
        <span className="text-[13px] t3">
          {t("names.cardShare", { share: Math.round(share), left })}
        </span>
        <span className="ml-auto flex gap-3">
          <button
            type="button"
            className="lnk text-[13px] !text-t3"
            disabled={working}
            onClick={onNext}
          >
            {t("names.skip")}
          </button>
          <button
            type="button"
            className="lnk text-[13px] !text-t3"
            disabled={working}
            onClick={onLater}
          >
            {t("names.later")}
          </button>
        </span>
      </div>
      <div className="text-[13px] t3">
        {t("names.askLead", { ask: ASK_LINES, enough: SAME_ENOUGH })}
      </div>
      <div className="flex items-center gap-2.5 mt-2.5 mb-0.5">
        <span
          role="progressbar"
          aria-label={t("names.progressLabel", { n: count, of: ASK_LINES })}
          aria-valuemin={0}
          aria-valuemax={ASK_LINES}
          aria-valuenow={count}
          className="flex-1 grid grid-cols-5 gap-[3px]"
        >
          {Array.from({ length: ASK_LINES }, (_, i) => (
            <i
              key={i}
              className={`block h-[3px] rounded-[2px] ${i < count ? "bg-t1" : "bg-s2"}`}
            />
          ))}
        </span>
        <span className="min-w-[2.6em] text-right text-[13px] tab">
          {t("names.progress", { n: count, of: ASK_LINES })}
        </span>
      </div>
      {/* What the answers say, read out when it comes. */}
      <div className="sr-only" aria-live="polite">
        {headline}
      </div>

      {line ? (
        <div className="mt-2 pt-2.5 border-t border-line">
          <div className="flex items-start gap-2.5">
            <TimeKey ms={line.start_ms} onListen={() => listen(line)} />
            <span className="flex-1">{`「${line.text}」`}</span>
          </div>
          <div className="mt-2.5 mb-1.5 text-[13px] font-normal t2">
            {t("names.whoSaid")}
          </div>
          <NamePicker
            meeting={meeting}
            current=""
            more={more}
            onPick={(name) => pick(name)}
          >
            <button
              type="button"
              className="lnk text-[13px] !text-t3 px-1"
              onClick={() => pick(null)}
            >
              {t("names.dontKnow")}
            </button>
          </NamePicker>
        </div>
      ) : null}

      {v.kind !== "asking" ? (
        <div className="mt-3 pt-3 border-t border-line">
          <div aria-hidden="true" className="text-[15px] font-semibold mb-0.5">
            {headline}
          </div>
          {v.kind === "decide" ? (
            <div className="t2 font-normal">
              {t("names.settleAsk", {
                speaker: shown,
                name: v.name,
                nameRo: withRo(v.name),
              })}
              {joins ? (
                <>
                  {" "}
                  {t("names.joins", {
                    name: joins.name,
                    nameWa: quotedWithWa(joins.name),
                  })}
                </>
              ) : null}
            </div>
          ) : v.kind === "mixed" ? (
            <div className="t2 font-normal">{t("names.mixedHint")}</div>
          ) : (
            <div className="t2 font-normal">{t("names.unknownHint")}</div>
          )}
          <div className="flex flex-wrap items-center gap-x-3.5 gap-y-2 mt-2.5 mb-1.5">
            <button
              ref={settleKey}
              type="button"
              className="btn solid !h-[28px] shadow-[0_0_0_4px_var(--redfaint)]"
              disabled={working}
              onClick={() =>
                v.kind === "none"
                  ? onNext()
                  : void settle(namingSteps(v), settleSaid)
              }
            >
              {v.kind === "decide"
                ? t("names.settle", { name: v.name, nameRo: withRo(v.name) })
                : v.kind === "mixed"
                  ? t("names.moveChosen")
                  : t("names.nextPerson")}
            </button>
            {v.kind === "decide" && canMore ? (
              <button
                type="button"
                className="lnk"
                disabled={working}
                onClick={() => setState((now) => askMore(now))}
              >
                {t("names.oneMore")}
              </button>
            ) : null}
          </div>
          {v.kind === "decide" ? (
            <div className="text-[12.5px] t3">
              {v.move.length > 0
                ? t("names.moveHint", {
                    list: v.move
                      .map((a) =>
                        t("names.moveOne", {
                          name: a.name ?? "",
                          nameRo: withRo(a.name ?? ""),
                          time: clock(a.line.start_ms),
                        }),
                      )
                      .join(", "),
                  })
                : t("names.followHint")}
            </div>
          ) : v.kind === "mixed" ? (
            <div className="text-[12.5px] t3">{t("names.mixedFoot")}</div>
          ) : null}
        </div>
      ) : null}

      {same ? (
        <div className="mt-3 text-[13.5px]">
          <span className="t2">
            {t("names.samePerson", { name: same.name })}
          </span>{" "}
          <button
            type="button"
            className="lnk"
            disabled={working}
            onClick={() =>
              void settle(
                [{ name: same.name, line: null }],
                t("names.merged", {
                  name: same.name,
                  nameRo: withRo(same.name),
                }),
              )
            }
          >
            {t("names.merge", { name: same.name })}
          </button>
        </div>
      ) : null}
      {trouble ? (
        <p role="alert" className="m-0 mt-1.5 text-[13px] rt">
          {trouble}
        </p>
      ) : null}

      {state.answers.length > 0 ? (
        <ol
          aria-label={t("names.answered")}
          className="m-0 mt-2.5 p-0 list-none border-t border-line"
        >
          {state.answers.map((a) => (
            <li
              key={a.line.start_ms}
              className="grid grid-cols-[auto_1fr_auto] items-baseline gap-2.5 py-1.5 border-b border-line text-[13px]"
            >
              <TimeKey
                ms={a.line.start_ms}
                onListen={() => listen(a.line)}
                className="!text-[12px]"
              />
              <span className="t2 font-normal truncate">{`「${a.line.text}」`}</span>
              <span className={a.name ? "font-semibold" : "t3 font-normal"}>
                {a.name ?? t("names.dontKnow")}
              </span>
            </li>
          ))}
        </ol>
      ) : null}
    </section>
  );
}

/**
 * Who a speaker is, from a line in the transcript: a name pressed is given
 * at once, to the whole meeting unless "from here" or "this line" is chosen.
 */
export function SpeakerNamer({
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
