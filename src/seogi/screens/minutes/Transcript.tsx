// The transcript, read as paragraphs, each line in its own span.

import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type Gap,
  type Meeting,
  type Segment,
  type WordFix,
} from "@/bindings";
import { TimeKey, useSpeakerName } from "../../components/common";
import { Marked } from "../../components/Found";
import {
  EditedMark,
  TextEdit,
  useConfirmRedo,
  WasWords,
} from "../../components/Edit";
import {
  FixedNote,
  FixMark,
  FixWas,
  PickKey,
  usePickedWords,
  WordFixPanel,
  type Fixed,
  type Picked,
} from "../../components/WordFix";
import {
  chars,
  distinctFixes,
  fixedByWord,
  pieces,
  quotedRo,
} from "../../lib/wordfix";
import { NS } from "../../i18n";
import { clock } from "../../lib/format";
import { type Player } from "../../lib/player";
import { isGibberish, koreanMeeting } from "../../lib/gibberish";
import { Mark } from "../../components/Mark";
import { useSeogi } from "../../store";
import { useGapText } from "./Parts";
import { SpeakerNamer, tonesOf } from "./Speakers";

interface Paragraph {
  startMs: number;
  endMs: number;
  speaker: number;
  /** The words as the user reads them: theirs where they fixed a line. */
  text: string;
  /** The words as transcribed. */
  original: string;
  /** Whether the user rewrote any of its lines whole. */
  edited: boolean;
  /** Whether any of its lines reads other than transcribed (rewritten, or
   *  a word fixed). */
  changed: boolean;
  unsure: boolean;
  /** Its lines, for showing fixed words where they are. */
  lines: Segment[];
}

/** `more` after `words` with a space, leaving out nothing said. */
const joined = (words: string, more: string) =>
  more.trim() ? (words ? `${words} ${more}` : more) : words;

/** Lines by the same speaker with no real pause between them, as one. */
function paragraphs(segments: Segment[]): Paragraph[] {
  const out: Paragraph[] = [];
  for (const line of segments) {
    const last = out[out.length - 1];
    const said = line.edited ?? line.text;
    const changed = line.edited !== undefined && line.edited !== null;
    const edited = changed && !fixedByWord(line);
    if (
      last &&
      last.speaker === line.speaker &&
      line.start_ms - last.endMs <= 2000
    ) {
      last.text = joined(last.text, said);
      last.original = joined(last.original, line.text);
      last.edited = last.edited || edited;
      last.changed = last.changed || changed;
      last.endMs = line.end_ms;
      last.unsure = last.unsure || (line.unsure ?? false);
      last.lines.push(line);
    } else {
      out.push({
        startMs: line.start_ms,
        endMs: line.end_ms,
        speaker: line.speaker,
        text: said.trim(),
        original: line.text,
        edited,
        changed,
        unsure: line.unsure ?? false,
        lines: [line],
      });
    }
  }
  return out;
}

export function Transcript({
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
  // The line (by its start) being fixed, and the fixed one whose words as
  // transcribed are shown.
  const [fixing, setFixing] = useState<number | null>(null);
  const [seeing, setSeeing] = useState<number | null>(null);
  // The whole transcript as first written, before any fix.
  const [asWritten, setAsWritten] = useState(false);
  // A fixed word whose fix is shown ("start|from|to"), the words being
  // fixed, the fix just made, and the minutes asked to be written again.
  const [seeingFix, setSeeingFix] = useState<string | null>(null);
  const [panel, setPanel] = useState<Picked | null>(null);
  const [fixed, setFixed] = useState<Fixed | null>(null);
  const [fixTrouble, setFixTrouble] = useState("");
  const redoMinutes = useConfirmRedo(meeting);
  const box = useRef<HTMLDivElement | null>(null);
  const [picked, dropPicked] = usePickedWords(
    box,
    asWritten || fixing !== null || panel !== null,
  );
  const current = useRef<HTMLDivElement | null>(null);
  const transcript = meeting.transcript;
  const marks = meeting.marks_ms ?? [];
  const gapText = useGapText();
  const gaps = [...(meeting.gaps ?? [])].sort((a, b) => a.at_ms - b.at_ms);
  const gapRow = (gap: Gap) => (
    <div
      key={`gap-${gap.at_ms}`}
      className="flex items-center gap-2.5 py-1.5 text-[13px] t3"
    >
      <TimeKey ms={gap.at_ms} onListen={(ms) => player.seek(ms)} icon={false} />
      <span>{gapText(gap)}</span>
    </div>
  );

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

  const anyEdited = lines.some((line) => line.changed);
  if (!transcript || lines.length === 0) return null;
  const nameOf = (id: number) =>
    transcript.speakers.find((speaker) => speaker.id === id)?.name ?? "";

  const putBack = async (from: number, to: number, fix: WordFix) => {
    const result = await commands.seogiUnfixWord(
      meeting.id,
      from,
      to,
      fix.from,
      fix.to,
    );
    if (result.status === "error") {
      setFixTrouble(t("failed", { reason: result.error }));
      return;
    }
    setSeeingFix(null);
    await refreshDetail();
  };

  return (
    <div
      ref={box}
      aria-label={t("minutes.transcript")}
      className="max-h-[70vh] overflow-y-auto pr-1"
    >
      <p className="mt-0 mb-2 text-[13.5px] t3">
        {asWritten ? t("edit.originalLead") : t("minutes.transcriptLead")}
        {anyEdited ? (
          <>
            {" "}
            <button
              type="button"
              className="lnk text-[13px] t2"
              aria-pressed={asWritten}
              onClick={() => {
                setAsWritten(!asWritten);
                setFixing(null);
                setSeeing(null);
                setSeeingFix(null);
                setPanel(null);
              }}
            >
              {asWritten ? t("edit.showEdited") : t("edit.showOriginal")}
            </button>
          </>
        ) : null}
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
        const shown = asWritten ? line.original : line.text;
        const noise = isGibberish(shown, korean);
        const row = (
          <div
            key={line.startMs}
            ref={on ? current : undefined}
            data-found={index === foundIndex ? "" : undefined}
            data-line={line.startMs}
            className={`tline py-2 px-2 -mx-2 rounded-[8px] ${
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
                className={`p-0 border-0 bg-transparent text-[13.5px] font-normal underline decoration-dotted decoration-[var(--t3)] underline-offset-4 cursor-pointer ${
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
              {asWritten || fixing === line.startMs ? null : (
                <span className={`acts ${on || marked ? "" : "ml-auto"}`}>
                  <button
                    type="button"
                    className="lnk"
                    aria-label={`${t("edit.fix")} ${clock(line.startMs)}`}
                    onClick={() => {
                      setFixing(line.startMs);
                      setSeeing(null);
                    }}
                  >
                    {t("edit.fix")}
                  </button>
                </span>
              )}
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
            {fixing === line.startMs ? (
              <div className="mt-1">
                <TextEdit
                  multiline
                  initial={line.text}
                  label={t("edit.lineLabel")}
                  onCancel={() => setFixing(null)}
                  onKeep={async (text) => {
                    const result = await commands.seogiEditLines(
                      meeting.id,
                      line.startMs,
                      line.endMs,
                      text,
                    );
                    if (result.status === "error") return result.error;
                    setFixing(null);
                    await refreshDetail();
                    return "";
                  }}
                />
              </div>
            ) : (
              <p
                title={noise ? t("minutes.noiseLine") : undefined}
                className={`mt-1 mb-0 text-[14.5px] leading-[1.7] ${
                  on ? "font-normal" : ""
                } ${noise ? "t3 opacity-60" : ""} ${line.unsure ? "underline decoration-dotted decoration-[var(--t3)] underline-offset-4" : ""}`}
              >
                {index === foundIndex && !asWritten ? (
                  <Marked text={line.text} words={words} />
                ) : asWritten ? (
                  shown
                ) : (
                  <LineWords paragraph={line} pick={panel} />
                )}
                {asWritten
                  ? null
                  : distinctFixes(line.lines).map((fix) => {
                      const key = `${line.startMs}|${fix.from}|${fix.to}`;
                      return (
                        <Fragment key={key}>
                          {" "}
                          <FixMark
                            fix={fix}
                            open={seeingFix === key}
                            onToggle={() =>
                              setSeeingFix(seeingFix === key ? null : key)
                            }
                          />
                        </Fragment>
                      );
                    })}
                {line.edited && !asWritten ? (
                  <>
                    {" "}
                    <EditedMark
                      open={seeing === line.startMs}
                      onToggle={() =>
                        setSeeing(seeing === line.startMs ? null : line.startMs)
                      }
                    />
                  </>
                ) : null}
              </p>
            )}
            {seeing === line.startMs && line.edited && !asWritten ? (
              <WasWords text={line.original} />
            ) : null}
            {asWritten
              ? null
              : distinctFixes(line.lines)
                  .filter(
                    (fix) =>
                      seeingFix === `${line.startMs}|${fix.from}|${fix.to}`,
                  )
                  .map((fix) => (
                    <FixWas
                      key="was"
                      fix={fix}
                      onPutBack={() =>
                        void putBack(
                          line.startMs,
                          line.lines[line.lines.length - 1].start_ms,
                          fix,
                        )
                      }
                    />
                  ))}
          </div>
        );
        // Where the microphone was away, between the lines around it.
        const gapsBefore = gaps.filter(
          (gap) =>
            gap.at_ms <= line.startMs &&
            (index === 0 || gap.at_ms > lines[index - 1].startMs),
        );
        return gapsBefore.length > 0 ? (
          <Fragment key={line.startMs}>
            {gapsBefore.map((gap) => gapRow(gap))}
            {row}
          </Fragment>
        ) : (
          row
        );
      })}
      {gaps
        .filter((gap) => gap.at_ms > lines[lines.length - 1].startMs)
        .map((gap) => gapRow(gap))}
      {picked && !panel ? (
        <PickKey
          picked={picked}
          onPress={() => {
            setPanel(picked);
            dropPicked();
            setFixed(null);
            setFixTrouble("");
          }}
        />
      ) : null}
      {panel ? (
        <WordFixPanel
          meeting={meeting}
          picked={panel}
          onCancel={() => setPanel(null)}
          onFixed={(done) => {
            setPanel(null);
            setFixed(done);
            window.getSelection()?.removeAllRanges();
            void refreshDetail();
          }}
        />
      ) : null}
      {fixed ? (
        <FixedNote
          fixed={fixed}
          trouble={fixTrouble}
          canRewrite={Boolean(meeting.minutes?.length)}
          onUndo={async () => {
            const result = await commands.seogiUnfixWords(
              meeting.id,
              fixed.outcome.before,
              fixed.from,
              fixed.to,
              fixed.outcome.remembered,
            );
            if (result.status === "error") {
              setFixTrouble(t("failed", { reason: result.error }));
              return;
            }
            setFixed(null);
            await refreshDetail();
          }}
          onRewrite={() => {
            setFixed(null);
            redoMinutes.ask("minutes", () => {
              void commands
                .seogiRetry(meeting.id, true)
                .then(() => refreshDetail());
            });
          }}
          onClose={() => {
            setFixed(null);
            setFixTrouble("");
          }}
        />
      ) : null}
      {redoMinutes.dialog}
    </div>
  );
}

/** A paragraph's words as read, each line in its own span (its start in
 *  `data-seg`, so words picked in it can be told where they are), fixed
 *  words underlined, and the words being fixed marked. */
function LineWords({
  paragraph,
  pick,
}: {
  paragraph: Paragraph;
  pick: Picked | null;
}) {
  const { t } = useTranslation(NS);
  const shown = paragraph.lines.filter((line) =>
    (line.edited ?? line.text).trim(),
  );
  return (
    <>
      {shown.map((line, index) => (
        <Fragment key={line.start_ms}>
          {index > 0 ? " " : null}
          <span data-seg={line.start_ms}>
            {pieces(
              line,
              pick && pick.atMs === line.start_ms
                ? { at: pick.at, n: chars(pick.words).length }
                : null,
            ).map((piece, at) =>
              piece.fix ? (
                <span
                  key={at}
                  className={`word-fixed${piece.picked ? " word-picked" : ""}`}
                  title={t("wordFix.was", {
                    from: piece.fix.from,
                    fromRo: quotedRo(piece.fix.from),
                  })}
                >
                  {piece.text}
                </span>
              ) : piece.picked ? (
                <mark key={at} className="word-picked">
                  {piece.text}
                </mark>
              ) : (
                <Fragment key={at}>{piece.text}</Fragment>
              ),
            )}
          </span>
        </Fragment>
      ))}
    </>
  );
}
