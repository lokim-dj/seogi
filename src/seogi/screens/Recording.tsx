// While a meeting is being recorded: a calm column with the clock, the
// meeting's name, the sound coming in, and two keys. What the minutes
// should know is folded away and may stay empty.

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import { commands, type Meeting, type MeetingInfo } from "@/bindings";
import { FlagGlyph, RightIcon, StopGlyph } from "../components/icons";
import { setMeetingInfo } from "../components/MeetingContext";
import { PeopleLine } from "../components/People";
import { NS } from "../i18n";
import { clock, keysShort } from "../lib/format";
import { useSeogi } from "../store";

const KINDS = ["consult", "vendor", "meeting"] as const;
/** Bars of sound shown, the newest at the right. */
const BARS = 150;
const NEWEST = 18;

export default function Recording() {
  const { t } = useTranslation(NS);
  const recording = useSeogi((s) => s.recording);
  const config = useSeogi((s) => s.config);
  const stopRecording = useSeogi((s) => s.stopRecording);
  const [meeting, setMeeting] = useState<Meeting | null>(null);
  const [trouble, setTrouble] = useState("");
  const [levels, setLevels] = useState<number[]>([]);
  const lastAt = useRef(0);
  const mac = platform() === "macos";
  const id = recording?.id ?? "";
  const markCount = recording?.marks_ms.length ?? 0;

  // What is known about this meeting (from the calendar, or typed here).
  useEffect(() => {
    if (!id) return;
    let alive = true;
    void commands.seogiMeeting(id).then((result) => {
      if (alive && result.status === "ok") setMeeting(result.data);
    });
    return () => {
      alive = false;
    };
  }, [id, markCount]);

  // The sound as it came in, a bar for each reading.
  const elapsed = recording?.elapsed_ms ?? 0;
  const level = recording?.level ?? 0;
  useEffect(() => {
    if (!recording || elapsed === lastAt.current) return;
    lastAt.current = elapsed;
    setLevels((now) => [...now.slice(-(BARS - 1)), level]);
  }, [recording, elapsed, level]);

  if (!recording) return null;

  const mark = async () => {
    const result = await commands.seogiMark();
    if (result.status === "error") setTrouble(result.error);
  };

  const stop = async () => {
    const error = await stopRecording();
    if (error) setTrouble(error);
  };

  const refresh = async () => {
    const fresh = await commands.seogiMeeting(id);
    if (fresh.status === "ok") setMeeting(fresh.data);
  };

  const setInfo = async (info: Partial<MeetingInfo>) => {
    const error = await setMeetingInfo(id, info);
    if (error) setTrouble(error);
    await refresh();
  };

  const event = meeting?.event;
  const bars = [...Array(Math.max(0, BARS - levels.length)).fill(0), ...levels];

  return (
    <div className="min-h-full relative pb-16">
      <div
        className="absolute inset-x-0 top-0 h-[2px] bg-red"
        aria-hidden="true"
      />
      <header className="a-bar">
        <span className="wm">{t("brand")}</span>
        <span className="tools">
          <span className={`ib ${recording.no_sound ? "rt" : "t3"}`}>
            {recording.no_sound ? t("recording.noSound") : recording.device}
          </span>
        </span>
      </header>

      <main className="a-recbig">
        <span role="status" className="state">
          {/* No blinking dot while nothing is being heard. */}
          {recording.reconnecting ? null : (
            <span className="live" aria-hidden="true" />
          )}
          {recording.reconnecting
            ? t("recording.reconnecting")
            : t("recording.on")}
        </span>
        <div className="a-timer">{clock(elapsed)}</div>
        {recording.reconnecting ? (
          <div className="t3 font-normal text-[13px] max-w-[420px] mb-2">
            {t("recording.reconnectingHint")}
          </div>
        ) : null}
        <input
          type="text"
          key={`title-${meeting?.title ?? ""}`}
          defaultValue={meeting?.title ?? ""}
          placeholder={t("recording.titlePlaceholder")}
          aria-label={t("recording.titleLabel")}
          onBlur={(change) => {
            const title = change.target.value.trim();
            if (title !== (meeting?.title ?? "")) void setInfo({ title });
          }}
          className="a-title ul"
        />
        <div className="t3 text-[13px] mt-1.5">
          {event ? t("recording.titleFromEvent") : t("recording.titleHint")}
        </div>
        {meeting ? (
          <PeopleLine
            meeting={meeting}
            onChanged={refresh}
            className="mt-3 justify-center"
          />
        ) : null}

        <div className="a-wavebox" role="img" aria-label={t("recording.level")}>
          <div className="wave">
            {bars.map((value, index) => (
              <i
                key={index}
                className={index >= BARS - NEWEST ? "new" : undefined}
                style={{
                  height: `${Math.max(8, Math.round(Math.sqrt(Math.min(1, value)) * 100))}%`,
                }}
              />
            ))}
          </div>
        </div>

        <div className="a-acts">
          <button type="button" className="btn" onClick={() => void mark()}>
            <FlagGlyph className="rt" />
            {t("recording.mark")}
            {config ? (
              <span className="k">{keysShort(config.mark_shortcut, mac)}</span>
            ) : null}
          </button>
          <button
            type="button"
            className="btn solid"
            onClick={() => void stop()}
          >
            <StopGlyph />
            {t("recording.stopShort")}
          </button>
        </div>
        <div className="t3 text-[13px] mt-2.5">{t("recording.stopHint")}</div>
        {trouble ? (
          <p role="alert" className="rt mt-3 mb-0 text-[13.5px]">
            {t("failed", { reason: trouble })}
          </p>
        ) : null}

        <div className="a-marks">
          {recording.marks_ms.map((ms, index) => (
            <div key={ms} className="m">
              <span className="tab">{clock(ms)}</span>
              <input
                type="text"
                aria-label={t("recording.memoLabel", { time: clock(ms) })}
                defaultValue={meeting?.mark_notes?.[index] ?? ""}
                placeholder={t("recording.memoPlaceholder")}
                onBlur={(change) =>
                  void commands.seogiSetMarkNote(index, change.target.value)
                }
                className="ul"
              />
            </div>
          ))}
          {markCount === 0 ? (
            <p className="m-0 t3 text-[13.5px]">{t("recording.noMarks")}</p>
          ) : null}

          <details className="mt-[18px]">
            <summary className="fold">
              <span>
                {t("recording.aboutFold")}{" "}
                <span className="t3">{t("recording.mayStayEmpty")}</span>
              </span>
              <span className="r">
                <RightIcon />
              </span>
            </summary>
            <div className="fold-body grid gap-3.5">
              <div>
                <div className="mb-1.5 text-[13.5px] t2">
                  {t("recording.kind")}
                </div>
                <div
                  className="seg"
                  role="group"
                  aria-label={t("recording.kind")}
                >
                  {KINDS.map((kind) => (
                    <button
                      key={kind}
                      type="button"
                      aria-pressed={meeting?.template_id === kind}
                      onClick={() => void setInfo({ template_id: kind })}
                    >
                      {t(`home.${kind}`)}
                    </button>
                  ))}
                </div>
                <div className="mt-1 text-[13px] t3">
                  {event && meeting?.template_id
                    ? t("recording.kindFromEvent")
                    : t("recording.kindAuto")}
                </div>
              </div>
              {meeting ? (
                <div>
                  <div className="mb-1.5 text-[13.5px] t2">
                    {t("recording.peopleField")}
                  </div>
                  <PeopleLine
                    meeting={meeting}
                    onChanged={refresh}
                    open
                    labelled={false}
                  />
                </div>
              ) : null}
              {meeting?.template_id === "consult" ? (
                <label className="block">
                  <span className="block text-[13.5px] t2">
                    {t("recording.customer")}
                  </span>
                  <input
                    type="text"
                    key={`customer-${meeting.customer ?? ""}`}
                    defaultValue={meeting.customer ?? ""}
                    onBlur={(change) =>
                      void setInfo({ customer: change.target.value })
                    }
                    className="ul"
                  />
                </label>
              ) : null}
              <label className="block">
                <span className="block text-[13.5px] t2">
                  {t("recording.placeField")}
                </span>
                <input
                  type="text"
                  key={`place-${meeting?.place ?? ""}`}
                  defaultValue={meeting?.place ?? ""}
                  placeholder={t("context.placeHint")}
                  onBlur={(change) =>
                    void setInfo({ place: change.target.value.trim() })
                  }
                  className="ul"
                />
              </label>
              <label className="block">
                <span className="block text-[13.5px] t2">
                  {t("recording.noteField")}
                </span>
                <textarea
                  key={`note-${meeting?.note ?? ""}`}
                  defaultValue={meeting?.note ?? ""}
                  rows={2}
                  placeholder={t("context.noteHint")}
                  onBlur={(change) =>
                    void setInfo({ note: change.target.value.trim() })
                  }
                  className="ul resize-y"
                />
              </label>
              <p className="m-0 text-[13px] t3">{t("recording.contextHint")}</p>
            </div>
          </details>
        </div>
      </main>
    </div>
  );
}
