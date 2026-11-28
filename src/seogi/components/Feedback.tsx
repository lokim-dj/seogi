// Feedback on a consultation, in the minutes for the team to read together:
// how the talking went (measured, not judged), then what the model noticed,
// every point with the times it rests on so anyone can hear it and say
// whether it is right.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type Meeting, type TalkStats } from "@/bindings";
import { NS } from "../i18n";
import { clock } from "../lib/format";
import { PlayIcon, TimeKey, useLength, useSpeakerName } from "./common";

/** Speakers who said less than this share are counted together. */
const SMALL_SHARE = 1;
const KINDS = [
  "good",
  "improve",
  "missed_questions",
  "hesitations",
  "next_time",
] as const;

export default function Feedback({
  meeting,
  onListen,
  onHear,
}: {
  meeting: Meeting;
  /** Plays from a moment. */
  onListen: (ms: number) => void;
  /** Plays a few seconds around a moment. */
  onHear: (ms: number) => void;
}) {
  const { t } = useTranslation(NS);
  const length = useLength();
  const speakerName = useSpeakerName();
  const [talk, setTalk] = useState<TalkStats | null>(null);
  const feedback = meeting.minutes?.[0]?.feedback;
  const speakers = meeting.transcript?.speakers ?? [];
  // Measured again whenever who said what changes.
  const shape = JSON.stringify(
    (meeting.transcript?.segments ?? []).map((s) => s.speaker),
  );

  useEffect(() => {
    void commands.seogiTalk(meeting.id).then((result) => {
      if (result.status === "ok") setTalk(result.data);
    });
  }, [meeting.id, shape]);

  if (!feedback) return null;
  const nameOf = (id: number) =>
    speakerName(speakers.find((s) => s.id === id)?.name ?? `화자 ${id}`);
  const big = (talk?.speakers ?? []).filter((s) => s.share >= SMALL_SHARE);
  const small = (talk?.speakers ?? []).filter((s) => s.share < SMALL_SHARE);
  const smallShare = small.reduce((n, s) => n + s.share, 0);

  return (
    <section aria-label={t("feedback.title")} className="fb">
      <p className="m-0 text-[13px] t3">{t("feedback.lead")}</p>

      {talk && big.length > 0 ? (
        <div className="grid gap-2">
          <div
            className="share"
            role="img"
            aria-label={big
              .map((s) => `${nameOf(s.speaker)} ${Math.round(s.share)}%`)
              .join(", ")}
          >
            {big.map((s) => (
              <Share key={s.speaker} name={nameOf(s.speaker)} share={s.share} />
            ))}
            {small.length > 0 ? (
              <Share
                name={t("feedback.others", {
                  n: small.length,
                  share: Math.round(smallShare),
                })}
                share={smallShare}
              />
            ) : null}
          </div>
          <details>
            <summary className="text-[13px] t3 cursor-pointer">
              {t("feedback.talkMore")}
            </summary>
            <ul className="m-0 mt-1.5 p-0 list-none grid gap-1 text-[13px] t2">
              {big.map((s) => (
                <li
                  key={s.speaker}
                  className="flex flex-wrap items-center gap-x-2"
                >
                  <span className="font-normal">{nameOf(s.speaker)}</span>
                  <span className="tab">
                    {t("feedback.speakerLine", {
                      share: Math.round(s.share),
                      length: length(s.talk_ms),
                      turns: s.turns,
                      questions: s.questions,
                      cutIns: s.cut_ins,
                    })}
                  </span>
                  <span className="t3">{t("feedback.longest")}</span>
                  <TimeKey ms={s.longest_at_ms} onListen={onListen} />
                  <span className="t3 tab">{length(s.longest_ms)}</span>
                </li>
              ))}
            </ul>
            {talk.moments.length > 0 ? (
              <div className="mt-2">
                <div className="mb-1 text-[13px] t3">
                  {t("feedback.moments")}
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {talk.moments.map((moment) => (
                    <button
                      key={moment.at_ms}
                      type="button"
                      onClick={() => onHear(moment.at_ms)}
                      aria-label={t("minutes.listen5From", {
                        time: clock(moment.at_ms),
                      })}
                      className="chip"
                    >
                      <PlayIcon size={8} />
                      {`${clock(moment.at_ms)} ${nameOf(moment.from)} → ${nameOf(moment.by)}`}
                    </button>
                  ))}
                </div>
              </div>
            ) : null}
            <p className="mt-2 mb-0 text-[13px] t3">{t("feedback.rough")}</p>
          </details>
        </div>
      ) : null}

      {KINDS.flatMap((kind) =>
        (feedback[kind] ?? []).map((point, index) => (
          <div key={`${kind}-${index}`} className="note">
            <span className="k">
              {index === 0 ? t(`feedback.kinds.${kind}`) : ""}
            </span>
            <span>{point.text}</span>
            <span className="c">
              {point.sources.map((source) => (
                <TimeKey key={source.ms} ms={source.ms} onListen={onListen} />
              ))}
            </span>
          </div>
        )),
      )}
      <p className="m-0 text-[13px] t3">{t("feedback.together")}</p>
    </section>
  );
}

/** One speaker's share of the talking, as a thin bar. */
function Share({ name, share }: { name: string; share: number }) {
  return (
    <>
      <span className="truncate">{name}</span>
      <span className="tr">
        <i style={{ width: `${Math.min(100, share)}%` }} />
      </span>
      <span className="t2 tab">{`${Math.round(share)}%`}</span>
    </>
  );
}
