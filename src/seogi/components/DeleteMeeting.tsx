// Deleting a meeting: its whole folder goes to the system's trash, after a
// question asked inside the window, and can be brought back for a minute
// from a note at the bottom (and from the trash after that).

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import { commands } from "@/bindings";
import { NS } from "../i18n";
import { useSeogi } from "../store";

/** What a meeting that may be deleted needs to show. */
export interface Deletable {
  id: string;
  title: string;
  status: string;
}

/** Whether a meeting is being recorded or worked on, and so kept. */
export function busy(meeting: Deletable): boolean {
  return meeting.status === "recording" || meeting.status === "working";
}

/** The question before a meeting goes to the trash. */
export function ConfirmDelete({
  meeting,
  onClose,
  onDeleted,
}: {
  meeting: Deletable;
  onClose: () => void;
  onDeleted?: () => void;
}) {
  const { t } = useTranslation(NS);
  const refreshMeetings = useSeogi((s) => s.refreshMeetings);
  const [trouble, setTrouble] = useState("");
  const [going, setGoing] = useState(false);

  useEffect(() => {
    const away = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", away);
    return () => window.removeEventListener("keydown", away);
  }, [onClose]);

  const remove = async () => {
    setGoing(true);
    const result = await commands.seogiDeleteMeeting(meeting.id);
    setGoing(false);
    if (result.status === "error") {
      setTrouble(
        result.error.includes("busy")
          ? t("trash.busy")
          : t("failed", { reason: result.error }),
      );
      return;
    }
    useSeogi.setState({
      trashed: { title: meeting.title || t("untitled"), at: Date.now() },
    });
    await refreshMeetings();
    onClose();
    onDeleted?.();
  };

  return (
    <div
      className="fixed inset-0 z-30 grid place-items-center bg-[var(--sh)] px-4"
      onClick={onClose}
    >
      <div
        role="alertdialog"
        aria-modal="true"
        aria-label={t("trash.title")}
        className="w-full max-w-[400px] p-5 rounded-[12px] bg-s1 shadow-[0_12px_32px_var(--sh)]"
        onClick={(event) => event.stopPropagation()}
      >
        <p className="m-0 text-[15.5px]">{t("trash.title")}</p>
        <p className="mt-1 mb-0 t2 truncate">
          {meeting.title || t("untitled")}
        </p>
        <p className="mt-3 mb-0 text-[13.5px] t3">
          {t(platform() === "windows" ? "trash.whatWindows" : "trash.what")}
        </p>
        {busy(meeting) ? (
          <p className="mt-3 mb-0 text-[13.5px] rt">{t("trash.busy")}</p>
        ) : null}
        {trouble ? (
          <p role="alert" className="mt-3 mb-0 text-[13.5px] rt">
            {trouble}
          </p>
        ) : null}
        <div className="flex justify-end gap-2 mt-5">
          <button type="button" className="btn" onClick={onClose}>
            {t("trash.cancel")}
          </button>
          <button
            type="button"
            className="btn solid"
            autoFocus
            disabled={going || busy(meeting)}
            onClick={() => void remove()}
          >
            {t("trash.move")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** A minute's note after a meeting went to the trash, to bring it back. */
export function TrashedNote() {
  const { t } = useTranslation(NS);
  const trashed = useSeogi((s) => s.trashed);
  const refreshMeetings = useSeogi((s) => s.refreshMeetings);
  const [trouble, setTrouble] = useState("");

  useEffect(() => {
    if (!trashed) return;
    setTrouble("");
    const left = Math.max(0, trashed.at + 60_000 - Date.now());
    const timer = window.setTimeout(
      () => useSeogi.setState({ trashed: null }),
      left,
    );
    return () => window.clearTimeout(timer);
  }, [trashed]);

  if (!trashed) return null;
  const undo = async () => {
    const result = await commands.seogiUndoDelete();
    if (result.status === "error") {
      setTrouble(t("trash.undoFailed"));
      return;
    }
    useSeogi.setState({ trashed: null });
    await refreshMeetings();
  };
  return (
    <div
      role="status"
      className="fixed left-1/2 bottom-16 z-30 -translate-x-1/2 flex items-center gap-4 max-w-[calc(100%-32px)] py-2.5 px-4 rounded-[10px] bg-s2 shadow-[0_8px_24px_var(--sh)] text-[13.5px]"
    >
      <span className="min-w-0 truncate">
        {trouble || t("trash.moved", { title: trashed.title })}
      </span>
      {trouble ? null : (
        <button type="button" className="lnk" onClick={() => void undo()}>
          {t("trash.undo")}
        </button>
      )}
      <button
        type="button"
        className="lnk"
        onClick={() => void commands.seogiOpenTrash()}
      >
        {t("trash.openTrash")}
      </button>
      <button
        type="button"
        className="lnk t3"
        onClick={() => useSeogi.setState({ trashed: null })}
      >
        {t("trash.close")}
      </button>
    </div>
  );
}
