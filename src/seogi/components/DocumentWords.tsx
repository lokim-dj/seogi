// Words from a meeting's documents: a quote, a drawing, a photo. The model
// that writes minutes picks the proper names; the user only ticks the ones
// to keep, and may add them to the vocabulary for every meeting as well.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { commands, type Meeting, type TermsFound } from "@/bindings";
import { NS } from "../i18n";
import { useSeogi } from "../store";
import { setMeetingInfo } from "./MeetingContext";

/** The kinds of document words are taken from. */
export const DOCUMENT_KINDS = [
  "pdf",
  "txt",
  "md",
  "csv",
  "tsv",
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
];

export function isDocument(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return DOCUMENT_KINDS.includes(ext);
}

export default function DocumentWords({
  meeting,
  onChanged,
}: {
  meeting: Meeting;
  onChanged: () => Promise<void> | void;
}) {
  const { t } = useTranslation(NS);
  const dropped = useSeogi((s) => s.droppedDocuments);
  const dropDocuments = useSeogi((s) => s.dropDocuments);
  const [reading, setReading] = useState(false);
  const [found, setFound] = useState<TermsFound | null>(null);
  const [ticked, setTicked] = useState<string[]>([]);
  const [everyMeeting, setEveryMeeting] = useState(false);
  const [note, setNote] = useState("");

  const read = async (paths: string[]) => {
    if (paths.length === 0) return;
    setReading(true);
    setNote("");
    setFound(null);
    const result = await commands.seogiTermsFromDocuments(meeting.id, paths);
    setReading(false);
    if (result.status === "error") {
      setNote(t("documents.failed", { reason: result.error }));
      return;
    }
    setFound(result.data);
    setTicked(result.data.terms);
  };

  // Documents dropped on the window while this meeting is open.
  useEffect(() => {
    if (!dropped || dropped.id !== meeting.id) return;
    dropDocuments(null);
    void read(dropped.paths);
  }, [dropped, meeting.id]);

  const choose = async () => {
    const picked = await open({
      multiple: true,
      filters: [{ name: t("documents.kinds"), extensions: DOCUMENT_KINDS }],
    });
    if (Array.isArray(picked)) await read(picked);
    else if (typeof picked === "string") await read([picked]);
  };

  const keep = async () => {
    const terms = [...(meeting.terms ?? [])];
    for (const term of ticked) if (!terms.includes(term)) terms.push(term);
    const error = await setMeetingInfo(meeting.id, { terms });
    if (error) {
      setNote(t("failed", { reason: error }));
      return;
    }
    let added = 0;
    if (everyMeeting && ticked.length > 0) {
      const result = await commands.seogiAddToVocabulary(ticked);
      if (result.status === "error") {
        setNote(t("failed", { reason: result.error }));
        return;
      }
      added = result.data;
    }
    setFound(null);
    setNote(
      everyMeeting
        ? t("documents.keptBoth", { n: ticked.length, added })
        : t("documents.kept", { n: ticked.length }),
    );
    await onChanged();
  };

  return (
    <div className="mt-2.5 pt-2.5 border-t border-cell">
      <div className="flex flex-wrap items-center gap-2.5">
        <button
          type="button"
          className="key key-small"
          disabled={reading}
          onClick={() => void choose()}
        >
          {reading ? t("documents.reading") : t("documents.choose")}
        </button>
        <span className="text-[14px] text-dim">
          {note || t("documents.hint")}
        </span>
      </div>

      {found ? (
        <div className="mt-2.5">
          {found.terms.length === 0 ? (
            <div className="text-dim">{t("documents.none")}</div>
          ) : (
            <>
              <div className="mb-1 text-[13.5px] t3">{t("documents.pick")}</div>
              <ul className="m-0 p-0 list-none grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-x-4 gap-y-0.5">
                {found.terms.map((term) => (
                  <li key={term}>
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={ticked.includes(term)}
                        onChange={(event) =>
                          setTicked((now) =>
                            event.target.checked
                              ? [...now, term]
                              : now.filter((other) => other !== term),
                          )
                        }
                        className="chk"
                      />
                      <span className="font-normal">{term}</span>
                    </label>
                  </li>
                ))}
              </ul>
              <div className="flex flex-wrap items-center gap-3 mt-2.5">
                <label className="flex items-center gap-2 cursor-pointer text-[13.5px]">
                  <input
                    type="checkbox"
                    checked={everyMeeting}
                    onChange={(event) => setEveryMeeting(event.target.checked)}
                    className="chk"
                  />
                  {t("documents.everyMeeting")}
                </label>
                <button
                  type="button"
                  className="key key-small"
                  disabled={ticked.length === 0}
                  onClick={() => void keep()}
                >
                  {t("documents.keep", { n: ticked.length })}
                </button>
                <button
                  type="button"
                  className="linkish"
                  onClick={() => setFound(null)}
                >
                  {t("documents.cancel")}
                </button>
              </div>
            </>
          )}
          {found.skipped.length > 0 ? (
            <ul className="mt-2 mb-0 pl-0 list-none text-[14px] text-dim">
              {found.skipped.map((skip) => (
                <li key={`${skip.file}-${skip.reason}`}>
                  {t("documents.skipped", {
                    file: skip.file,
                    why: t(`documents.why.${skip.reason}`, {
                      defaultValue: t("documents.why.unreadable"),
                    }),
                  })}
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
