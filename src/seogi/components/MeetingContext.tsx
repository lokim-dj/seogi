// What the user tells about one meeting: who was there, the site, a note,
// and this meeting's own words. With the vocabulary (for every meeting) it
// steers the speech model and the minutes. Everything here can be left
// empty; a calendar event fills the people and the place by itself.

import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { commands, type Meeting, type MeetingInfo } from "@/bindings";
import { NS } from "../i18n";
import { useConfirmRedo } from "./Edit";
import { PeopleLine } from "./People";

const NOTHING: MeetingInfo = {
  title: null,
  template_id: null,
  customer: null,
  people: null,
  place: null,
  note: null,
  terms: null,
};

/** Names typed with commas (or the Korean list marks) between them. */
export function splitNames(text: string): string[] {
  return text
    .split(/[,，、\n]/)
    .map((name) => name.trim())
    .filter(Boolean);
}

/** Saves part of a meeting's information; the rest is left as it is. */
export async function setMeetingInfo(
  id: string,
  info: Partial<MeetingInfo>,
): Promise<string | null> {
  const result = await commands.seogiSetMeetingInfo(id, {
    ...NOTHING,
    ...info,
  });
  return result.status === "error" ? result.error : null;
}

export default function MeetingContext({
  meeting,
  working,
  onChanged,
  documents,
}: {
  meeting: Meeting;
  working: boolean;
  onChanged: () => Promise<void> | void;
  /** The part that takes words from documents, when there is one. */
  documents?: ReactNode;
}) {
  const { t } = useTranslation(NS);
  const [note, setNote] = useState("");
  const terms = meeting.terms ?? [];

  const save = async (info: Partial<MeetingInfo>) => {
    const error = await setMeetingInfo(meeting.id, info);
    setNote(error ? t("failed", { reason: error }) : t("context.saved"));
    await onChanged();
  };

  // Asks first when writing again would undo what the user changed.
  const redo = useConfirmRedo(meeting);
  const again = (fromSpeech: boolean) =>
    redo.ask(fromSpeech ? "speech" : "minutes", () => void run(fromSpeech));
  const run = async (fromSpeech: boolean) => {
    const result = fromSpeech
      ? await commands.seogiRetranscribe(meeting.id)
      : await commands.seogiRetry(meeting.id, true);
    setNote(
      result.status === "error"
        ? t("failed", { reason: result.error })
        : t("context.queued"),
    );
    await onChanged();
  };

  const row = "cell-head w-[96px] !py-2.5 align-top";
  return (
    <section aria-label={t("context.title")}>
      <p className="mt-0 mb-2 max-w-[66ch] text-[13.5px] leading-[1.6] t3">
        {t("context.lead")}
      </p>
      <table className="w-full border-collapse text-[14px]">
        <tbody>
          <tr>
            <th scope="row" className={row}>
              {t("context.people")}
            </th>
            <td className="cell !py-2.5">
              <PeopleLine meeting={meeting} onChanged={onChanged} open />
            </td>
          </tr>
          <tr>
            <th scope="row" className={row}>
              {t("context.place")}
            </th>
            <td className="cell !py-1.5">
              <input
                type="text"
                key={`place-${meeting.place ?? ""}`}
                defaultValue={meeting.place ?? ""}
                onBlur={(event) => {
                  const place = event.target.value.trim();
                  if (place !== (meeting.place ?? "")) void save({ place });
                }}
                aria-label={t("context.place")}
                placeholder={t("context.placeHint")}
                className="field field-soft"
              />
            </td>
          </tr>
          <tr>
            <th scope="row" className={row}>
              {t("context.note")}
            </th>
            <td className="cell !py-1.5">
              <textarea
                key={`note-${meeting.note ?? ""}`}
                defaultValue={meeting.note ?? ""}
                rows={2}
                onBlur={(event) => {
                  const text = event.target.value.trim();
                  if (text !== (meeting.note ?? "")) void save({ note: text });
                }}
                aria-label={t("context.note")}
                placeholder={t("context.noteHint")}
                className="field resize-y"
              />
            </td>
          </tr>
          <tr>
            <th scope="row" className={row}>
              {t("context.terms")}
            </th>
            <td className="cell !py-1.5">
              {terms.length === 0 ? (
                <div className="text-dim">{t("context.noTerms")}</div>
              ) : (
                <ul className="m-0 p-0 list-none flex flex-wrap gap-x-4 gap-y-1">
                  {terms.map((term) => (
                    <li key={term} className="flex items-center gap-1.5">
                      <span className="font-medium">{term}</span>
                      <button
                        type="button"
                        className="linkish text-[14px]"
                        aria-label={t("context.removeTerm", { term })}
                        onClick={() =>
                          void save({
                            terms: terms.filter((other) => other !== term),
                          })
                        }
                      >
                        {t("context.remove")}
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              {documents}
            </td>
          </tr>
        </tbody>
      </table>
      <div className="flex flex-wrap items-center gap-2.5 mt-3">
        {meeting.transcript ? (
          <button
            type="button"
            className="key key-small"
            disabled={working}
            onClick={() => again(false)}
          >
            {t("context.rewrite")}
          </button>
        ) : null}
        <button
          type="button"
          className="key key-small"
          disabled={working || meeting.status === "recording"}
          onClick={() => again(true)}
        >
          {t("context.retranscribe")}
        </button>
        <span role="status" className="text-[14px] text-dim">
          {note || t("context.againHint")}
        </span>
      </div>
      {redo.dialog}
    </section>
  );
}
