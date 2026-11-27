// Changing a task or a transcript line in place: a box with the words, and
// "그만두기 · 고친 대로 두기" beside it. Enter keeps, Escape leaves it as it
// was. What was said and transcribed stays in the meeting's record; this
// only gives the user's words.

import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";
import type { Meeting } from "@/bindings";
import { NS } from "../i18n";

export function TextEdit({
  initial,
  label,
  placeholder,
  multiline = false,
  onCancel,
  onKeep,
}: {
  initial: string;
  label: string;
  placeholder?: string;
  /** A transcript paragraph: a box several lines tall. */
  multiline?: boolean;
  onCancel: () => void;
  /** Keeps the words; resolves to an error to show, or "" when kept. */
  onKeep: (text: string) => Promise<string>;
}) {
  const { t } = useTranslation(NS);
  const [value, setValue] = useState(initial);
  const [saving, setSaving] = useState(false);
  const [trouble, setTrouble] = useState("");
  const box = useRef<HTMLInputElement | HTMLTextAreaElement | null>(null);

  useEffect(() => {
    const field = box.current;
    if (!field) return;
    field.focus();
    // The cursor at the end, where a fix usually goes on.
    field.setSelectionRange(field.value.length, field.value.length);
  }, []);

  // A paragraph's box is as tall as its words, so all of them show.
  // Measured again when its width changes (a fold opening, the window).
  useLayoutEffect(() => {
    const field = box.current;
    if (!multiline || !(field instanceof HTMLTextAreaElement)) return;
    const fit = () => {
      field.style.height = "auto";
      field.style.height = `${field.scrollHeight + 2}px`;
    };
    fit();
    let width = field.clientWidth;
    const watch = new ResizeObserver(() => {
      if (field.clientWidth === width) return;
      width = field.clientWidth;
      fit();
    });
    watch.observe(field);
    return () => watch.disconnect();
  }, [multiline, value]);

  const keep = async () => {
    const text = value.trim();
    if (saving) return;
    if (text === initial.trim()) {
      onCancel();
      return;
    }
    if (!text && !multiline) return;
    setSaving(true);
    const error = await onKeep(text);
    setSaving(false);
    if (error) setTrouble(t("failed", { reason: error }));
  };

  const onKeyDown = (
    event: KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>,
  ) => {
    // A Korean syllable still being put together takes Enter itself.
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
    } else if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void keep();
    }
  };

  const keys = (
    <span className="edit-keys">
      <button type="button" className="lnk t3" onClick={onCancel}>
        {t("edit.cancel")}
      </button>
      <button
        type="button"
        className="lnk"
        disabled={saving || (!multiline && !value.trim())}
        onClick={() => void keep()}
      >
        {t("edit.keep")}
      </button>
    </span>
  );

  return (
    <span className={multiline ? "edit edit-many" : "edit"}>
      {multiline ? (
        <textarea
          ref={(node) => {
            box.current = node;
          }}
          className="field"
          aria-label={label}
          value={value}
          rows={2}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={onKeyDown}
        />
      ) : (
        <input
          ref={(node) => {
            box.current = node;
          }}
          type="text"
          className="field"
          aria-label={label}
          placeholder={placeholder}
          value={value}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={onKeyDown}
        />
      )}
      {keys}
      {trouble ? (
        <span className="block mt-1 text-[13px] rt">{trouble}</span>
      ) : null}
    </span>
  );
}

/** The small "고침" after words the user changed; pressed, it shows the
 *  words as they were. */
export function EditedMark({
  open,
  onToggle,
}: {
  open: boolean;
  onToggle: () => void;
}) {
  const { t } = useTranslation(NS);
  return (
    <button
      type="button"
      className="edit-mark"
      aria-expanded={open}
      title={t("edit.editedHint")}
      onClick={onToggle}
    >
      {t("edit.edited")}
    </button>
  );
}

/** The words as they were before the user changed them. */
export function WasWords({ text }: { text: string }) {
  const { t } = useTranslation(NS);
  return (
    <p className="edit-was">
      <span>{t("edit.original")}</span> {`「${text}」`}
    </p>
  );
}

/** What the user changed that writing again would put back: tasks whose
 *  words were changed, added or taken out, and (when the recording is
 *  transcribed again) fixed transcript lines, a run of fixed lines by one
 *  speaker counted once. */
export function lostBy(meeting: Meeting, redo: Redo) {
  const sections = meeting.minutes?.[0]?.sections ?? [];
  const entries = sections.flatMap((section) => section.entries);
  const segments = meeting.transcript?.segments ?? [];
  // A line the user's vocabulary alone fixed is fixed again when the
  // recording is transcribed again: nothing of the user's goes.
  const fixed = (index: number) => {
    const line = segments[index];
    if (!line || line.edited === undefined || line.edited === null)
      return false;
    const fixes = line.fixes ?? [];
    return !(fixes.length > 0 && fixes.every((fix) => fix.auto));
  };
  return {
    changed: entries.filter((entry) => entry.original && !entry.added).length,
    added: entries.filter((entry) => entry.added).length,
    removed: sections.reduce((n, s) => n + (s.removed ?? []).length, 0),
    lines:
      redo === "speech"
        ? segments.filter(
            (line, index) =>
              fixed(index) &&
              !(
                fixed(index - 1) && segments[index - 1].speaker === line.speaker
              ),
          ).length
        : 0,
  };
}

/** Writing the minutes again, or transcribing the recording again. */
export type Redo = "minutes" | "speech";

/** Asks before writing again would undo what the user changed; goes
 *  straight on when nothing was changed. Render `dialog` somewhere that
 *  stays on the screen. */
export function useConfirmRedo(meeting: Meeting | null | undefined) {
  const [pending, setPending] = useState<{ redo: Redo; go: () => void } | null>(
    null,
  );
  const ask = (redo: Redo, go: () => void) => {
    const lost = meeting ? lostBy(meeting, redo) : null;
    if (!lost || !Object.values(lost).some((n) => n > 0)) {
      go();
      return;
    }
    setPending({ redo, go });
  };
  const dialog =
    pending && meeting ? (
      <ConfirmRedo
        redo={pending.redo}
        lost={lostBy(meeting, pending.redo)}
        keepsLines={
          pending.redo === "minutes" && lostBy(meeting, "speech").lines > 0
        }
        onCancel={() => setPending(null)}
        onGo={() => {
          const go = pending.go;
          setPending(null);
          go();
        }}
      />
    ) : null;
  return { ask, dialog };
}

function ConfirmRedo({
  redo,
  lost,
  keepsLines,
  onCancel,
  onGo,
}: {
  redo: Redo;
  lost: ReturnType<typeof lostBy>;
  /** The minutes are written again from the fixed transcript lines. */
  keepsLines: boolean;
  onCancel: () => void;
  onGo: () => void;
}) {
  const { t } = useTranslation(NS);
  useEffect(() => {
    const away = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", away);
    return () => window.removeEventListener("keydown", away);
  }, [onCancel]);
  const what = [
    lost.changed ? t("edit.lostChanged", { n: lost.changed }) : "",
    lost.added ? t("edit.lostAdded", { n: lost.added }) : "",
    lost.removed ? t("edit.lostRemoved", { n: lost.removed }) : "",
    lost.lines ? t("edit.lostLines", { n: lost.lines }) : "",
  ]
    .filter(Boolean)
    .join(", ");
  const title = t(redo === "speech" ? "edit.redoSpeech" : "edit.redoMinutes");
  return (
    <div
      className="fixed inset-0 z-30 grid place-items-center bg-[var(--sh)] px-4"
      onClick={onCancel}
    >
      <div
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        className="w-full max-w-[420px] p-5 rounded-[12px] bg-s1 shadow-[0_12px_32px_var(--sh)]"
        onClick={(event) => event.stopPropagation()}
      >
        <p className="m-0 text-[15.5px]">{title}</p>
        <p className="mt-2 mb-0 text-[14px] leading-[1.6] t2">
          {t("edit.lostWill", { what })}
        </p>
        {keepsLines ? (
          <p className="mt-2 mb-0 text-[13.5px] leading-[1.6] t3">
            {t("edit.keepsLines")}
          </p>
        ) : null}
        <div className="flex justify-end gap-2 mt-5">
          <button type="button" className="btn" onClick={onCancel}>
            {t("edit.cancel")}
          </button>
          <button type="button" className="btn solid" autoFocus onClick={onGo}>
            {t(redo === "speech" ? "edit.redoSpeechGo" : "edit.redoMinutesGo")}
          </button>
        </div>
      </div>
    </div>
  );
}
