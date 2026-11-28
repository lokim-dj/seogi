// Fixing a misheard word in the transcript: words picked by dragging get a
// small "틀린 말 고치기" just above them; pressed, a panel asks for the right
// word (typed, or one of the vocabulary's), says how many places in this
// meeting have the same words, and fixes them together. "다음 회의에도"
// starts on and puts the words in the user's vocabulary, so meetings from
// now on are fixed as they are transcribed. The words as transcribed, the
// original recording and 전사.txt stay; the minutes are not written again
// on their own.

import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type RefObject,
} from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type Meeting,
  type WordFix,
  type WordFixOutcome,
  type WordList,
} from "@/bindings";
import { NS } from "../i18n";
import { clock } from "../lib/format";
import {
  chars,
  knownWord,
  quotedRo,
  quotedTopic,
  suggestions,
} from "../lib/wordfix";

/** Words picked in one line: where, and where on the screen. */
export interface Picked {
  /** The line they are in, by its start. */
  atMs: number;
  /** The first picked character in the line as read. */
  at: number;
  words: string;
  /** The picked words' box on the screen. */
  top: number;
  bottom: number;
  left: number;
  right: number;
}

/** The longest stretch offered for fixing: a word or a short phrase. */
const MOST_LETTERS = 40;

/** Words picked by dragging inside `box`, within one line (an element with
 *  `data-seg`, the line's start); null when nothing is picked there. */
export function usePickedWords(
  box: RefObject<HTMLElement | null>,
  off: boolean,
): [Picked | null, () => void] {
  const [picked, setPicked] = useState<Picked | null>(null);
  useEffect(() => {
    if (off) {
      setPicked(null);
      return;
    }
    const lineOf = (node: Node | null) =>
      (node instanceof Element
        ? node
        : node?.parentElement
      )?.closest<HTMLElement>("[data-seg]") ?? null;
    const look = () => {
      const selection = window.getSelection();
      if (!selection || selection.isCollapsed || selection.rangeCount === 0) {
        setPicked(null);
        return;
      }
      const range = selection.getRangeAt(0);
      const line = lineOf(range.startContainer);
      if (
        !line ||
        line !== lineOf(range.endContainer) ||
        !box.current?.contains(line)
      ) {
        setPicked(null);
        return;
      }
      const raw = range.toString();
      const words = raw.trim();
      if (!words || /\n/.test(words) || chars(words).length > MOST_LETTERS) {
        setPicked(null);
        return;
      }
      const before = document.createRange();
      before.setStart(line, 0);
      before.setEnd(range.startContainer, range.startOffset);
      const lead = raw.length - raw.trimStart().length;
      const at =
        chars(before.toString()).length + chars(raw.slice(0, lead)).length;
      const rect = range.getBoundingClientRect();
      setPicked({
        atMs: Number(line.dataset.seg),
        at,
        words,
        top: rect.top,
        bottom: rect.bottom,
        left: rect.left,
        right: rect.right,
      });
    };
    document.addEventListener("selectionchange", look);
    // The key follows the words when the page or the transcript scrolls.
    document.addEventListener("scroll", look, true);
    window.addEventListener("resize", look);
    return () => {
      document.removeEventListener("selectionchange", look);
      document.removeEventListener("scroll", look, true);
      window.removeEventListener("resize", look);
    };
  }, [box, off]);
  return [picked, () => setPicked(null)];
}

/** The small key just above picked words. */
export function PickKey({
  picked,
  onPress,
}: {
  picked: Picked;
  onPress: () => void;
}) {
  const { t } = useTranslation(NS);
  const key = useRef<HTMLButtonElement | null>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => setWidth(key.current?.offsetWidth ?? 0), []);
  // From the keyboard: Enter (or Shift+F10) while words are picked.
  useEffect(() => {
    const press = (event: globalThis.KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (
        target?.closest("input, textarea, select, button, a, [contenteditable]")
      )
        return;
      if (event.key === "Enter" || (event.key === "F10" && event.shiftKey)) {
        event.preventDefault();
        onPress();
      }
    };
    window.addEventListener("keydown", press);
    return () => window.removeEventListener("keydown", press);
  }, [onPress]);
  const middle = (picked.left + picked.right) / 2;
  const left = Math.max(
    8,
    Math.min(middle - width / 2, window.innerWidth - width - 8),
  );
  // Above the words, or under them at the window's top.
  const top = picked.top > 48 ? picked.top - 40 : picked.bottom + 8;
  return (
    <button
      ref={key}
      type="button"
      className="pick-key"
      style={{ top, left }}
      aria-keyshortcuts="Enter Shift+F10"
      // Pressing must not drop the picked words before the press lands.
      onMouseDown={(event) => event.preventDefault()}
      onClick={onPress}
    >
      {t("wordFix.pick")}
    </button>
  );
}

/** The panel that fixes picked words. */
export function WordFixPanel({
  meeting,
  picked,
  onCancel,
  onFixed,
}: {
  meeting: Meeting;
  picked: Picked;
  onCancel: () => void;
  onFixed: (fixed: Fixed) => void;
}) {
  const { t } = useTranslation(NS);
  const [to, setTo] = useState("");
  const [remember, setRemember] = useState(true);
  const [places, setPlaces] = useState<number | null>(null);
  const [vocabulary, setVocabulary] = useState<WordList | null>(null);
  const [saving, setSaving] = useState(false);
  const [trouble, setTrouble] = useState("");
  const panel = useRef<HTMLDivElement | null>(null);
  const [height, setHeight] = useState(0);
  const from = picked.words;

  useEffect(() => {
    let live = true;
    void commands.seogiVocabulary().then((result) => {
      if (live && result.status === "ok") setVocabulary(result.data);
    });
    return () => {
      live = false;
    };
  }, []);

  // Counted for the right word as it is written: where the meeting has it
  // already, the same letters are left alone, so the count is what the
  // fix changes.
  const right = to.trim();
  useEffect(() => {
    let live = true;
    const count = window.setTimeout(
      () =>
        void commands
          .seogiWordPlaces(meeting.id, picked.atMs, picked.at, from, right)
          .then((result) => {
            if (live) setPlaces(result.status === "ok" ? result.data : 1);
          }),
      right ? 250 : 0,
    );
    return () => {
      live = false;
      window.clearTimeout(count);
    };
  }, [meeting.id, picked.atMs, picked.at, from, right]);

  useLayoutEffect(() => {
    setHeight(panel.current?.offsetHeight ?? 0);
  }, [vocabulary, places, to, trouble]);

  useEffect(() => {
    const away = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", away);
    return () => window.removeEventListener("keydown", away);
  }, [onCancel]);

  const words = vocabulary?.words ?? [];
  const offered = suggestions(from, to, words);
  // Why the vocabulary cannot learn these words, if it cannot.
  const other = right
    ? words.find(
        (word) =>
          word.term !== right && word.misheard.some((m) => m.form === from),
      )
    : undefined;
  const cannotLearn =
    chars(from).length < 2
      ? t("wordFix.nextShort")
      : knownWord(from, words)
        ? t("wordFix.nextKnown")
        : vocabulary && !vocabulary.writable
          ? t("wordFix.nextReadOnly")
          : other
            ? t("wordFix.nextTaken", {
                from,
                other: other.term,
                fromTopic: quotedTopic(from),
                otherRo: quotedRo(other.term),
              })
            : "";
  const count = places ?? 1;

  const fix = async () => {
    if (!right || right === from || saving) return;
    setSaving(true);
    setTrouble("");
    const learn = remember && !cannotLearn;
    const result = await commands.seogiFixWord(
      meeting.id,
      picked.atMs,
      picked.at,
      from,
      right,
      learn,
    );
    setSaving(false);
    if (result.status === "error") {
      setTrouble(t("failed", { reason: result.error }));
      return;
    }
    onFixed({ outcome: result.data, from, to: right });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      void fix();
    }
  };

  // Under the words, or above them when the window ends first.
  const width = Math.min(380, window.innerWidth - 32);
  const left = Math.max(
    16,
    Math.min(picked.left - 24, window.innerWidth - width - 16),
  );
  const below = picked.bottom + 10;
  const top =
    height && below + height > window.innerHeight - 16
      ? Math.max(16, picked.top - 10 - height)
      : below;

  const facts = [
    right && knownWord(right, words) ? t("wordFix.inVocabulary") : "",
    places === null
      ? t("wordFix.counting")
      : places > 1
        ? t("wordFix.morePlaces", { n: places - 1 })
        : t("wordFix.onlyHere"),
  ].filter(Boolean);

  return (
    <div
      ref={panel}
      role="dialog"
      aria-label={t("wordFix.panel")}
      className="fix-panel"
      style={{ top, left, width }}
    >
      <p className="fix-label">{t("wordFix.heard")}</p>
      <p className="m-0 text-[15px] font-semibold">
        {`「${from}」`}{" "}
        <span className="t3 text-[13.5px]">{clock(picked.atMs)}</span>
      </p>
      <label className="fix-label mt-3 block" htmlFor="fix-right">
        {t("wordFix.right")}
      </label>
      <input
        id="fix-right"
        type="text"
        className="field w-full text-[15px]"
        autoFocus
        value={to}
        placeholder={t("wordFix.rightPlaceholder")}
        onChange={(event) => setTo(event.target.value)}
        onKeyDown={onKeyDown}
      />
      {offered.length > 0 ? (
        <div
          role="group"
          aria-label={t("wordFix.fromVocabulary")}
          className="fix-offers"
        >
          {offered.map((term) => (
            <button
              key={term}
              type="button"
              className="lnk"
              onClick={() => setTo(term)}
            >
              {term}
            </button>
          ))}
        </div>
      ) : null}
      <p className="mt-2 mb-0 text-[13.5px] t3">{facts.join(" · ")}</p>
      <label
        className={`fix-next ${cannotLearn ? "opacity-70" : "cursor-pointer"}`}
      >
        <input
          type="checkbox"
          className="mt-[3px] accent-[var(--t1)]"
          checked={remember && !cannotLearn}
          disabled={Boolean(cannotLearn)}
          onChange={(event) => setRemember(event.target.checked)}
        />
        <span>
          <span className="block text-[14.5px]">{t("wordFix.next")}</span>
          <span className="block text-[13px] t3">
            {cannotLearn || t("wordFix.nextHint")}
          </span>
        </span>
      </label>
      {trouble ? <p className="mt-2 mb-0 text-[13px] rt">{trouble}</p> : null}
      <div className="flex justify-end items-center gap-3 mt-4">
        <button type="button" className="lnk t3" onClick={onCancel}>
          {t("edit.cancel")}
        </button>
        <button
          type="button"
          className="btn solid"
          disabled={!right || right === from || saving}
          onClick={() => void fix()}
        >
          {count > 1 ? t("wordFix.goMany", { n: count }) : t("wordFix.go")}
        </button>
      </div>
    </div>
  );
}

/** A fix just made, as the note under the transcript tells it. */
export interface Fixed {
  outcome: WordFixOutcome;
  from: string;
  to: string;
}

/** The note after a fix: how many places, undo, and the minutes to write
 *  again if the user wants the fixed words there too. */
export function FixedNote({
  fixed,
  trouble,
  canRewrite,
  onUndo,
  onRewrite,
  onClose,
}: {
  fixed: Fixed;
  /** Why the undo did not work, if it did not. */
  trouble: string;
  canRewrite: boolean;
  onUndo: () => void;
  onRewrite: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation(NS);
  const n = fixed.outcome.places;
  const said = t(
    fixed.outcome.remembered ? "wordFix.doneRemembered" : "wordFix.done",
    { n, to: fixed.to, toRo: quotedRo(fixed.to) },
  );
  return (
    <div role="status" className="fix-note">
      <p className="m-0 flex flex-wrap items-baseline gap-x-2">
        <span>{said}</span>
        <span className="t3">·</span>
        <button type="button" className="lnk" onClick={onUndo}>
          {t("wordFix.undo")}
        </button>
        <button type="button" className="lnk t3 ml-auto" onClick={onClose}>
          {t("wordFix.close")}
        </button>
      </p>
      <p className="mt-1.5 mb-0 text-[13.5px] t3">
        {canRewrite ? (
          <>
            {t("wordFix.rewriteLead")}{" "}
            <button type="button" className="lnk t2" onClick={onRewrite}>
              {t("wordFix.rewrite")}
            </button>
            {" · "}
          </>
        ) : null}
        {t("wordFix.recordStays")}
      </p>
      {trouble ? <p className="mt-1.5 mb-0 text-[13px] rt">{trouble}</p> : null}
    </div>
  );
}

/** "고침 · 「원래 말」" after a paragraph with a fixed word; pressed, it says
 *  who fixed it and offers the words as transcribed back. */
export function FixMark({
  fix,
  open,
  onToggle,
}: {
  fix: WordFix;
  open: boolean;
  onToggle: () => void;
}) {
  const { t } = useTranslation(NS);
  return (
    <button
      type="button"
      className="edit-mark"
      aria-expanded={open}
      title={t("wordFix.markHint")}
      onClick={onToggle}
    >
      {t("wordFix.mark", { from: fix.from })}
    </button>
  );
}

/** What a fix was, and the key that puts the words as transcribed back. */
export function FixWas({
  fix,
  onPutBack,
}: {
  fix: WordFix;
  onPutBack: () => void;
}) {
  const { t } = useTranslation(NS);
  return (
    <p className="edit-was">
      {t("wordFix.was", { from: fix.from, fromRo: quotedRo(fix.from) })}
      {" · "}
      {fix.auto ? t("wordFix.byVocabulary") : t("wordFix.byYou")}
      {" · "}
      <button type="button" className="lnk t2" onClick={onPutBack}>
        {t("wordFix.putBack")}
      </button>
    </p>
  );
}
