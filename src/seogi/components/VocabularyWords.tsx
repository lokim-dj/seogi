// The vocabulary in the settings (설정 › 녹음 › 용어집): one list, each word
// with the ways the speech model mishears it ("이렇게 잘못 들려요") and, for
// a way learnt in a meeting, which meeting and when. Words are added, changed
// and taken out here; the list is the user's own file on this computer
// (src-tauri/src/seogi/vocab.rs), never anywhere else.

import { useEffect, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { commands, type Misheard, type Word, type WordList } from "@/bindings";
import { NS } from "../i18n";
import { daysAgo, friendlyPath } from "../lib/format";

/** Rows shown before "N개 더". */
const FIRST_ROWS = 6;

/** The ways a word is misheard as the user types them: comma separated. */
const formsOf = (text: string) =>
  text
    .split(/[,，、]/)
    .map((form) => form.trim())
    .filter(Boolean);

export default function VocabularyWords({
  path,
  onBack,
  onPick,
  onChanged,
}: {
  /** The vocabulary file or folder in use. */
  path: string;
  onBack: () => void;
  /** Uses another folder (`true`) or a single file. */
  onPick: (directory: boolean) => void;
  /** The count elsewhere in the settings follows a change. */
  onChanged: () => void;
}) {
  const { t, i18n } = useTranslation(NS);
  const [list, setList] = useState<WordList | null>(null);
  const [trouble, setTrouble] = useState("");
  const [query, setQuery] = useState("");
  const [all, setAll] = useState(false);
  // The word being changed (by its name), and "" for none.
  const [editing, setEditing] = useState("");
  const [removed, setRemoved] = useState<Word | null>(null);

  const load = async () => {
    const result = await commands.seogiVocabulary();
    if (result.status === "ok") {
      setList(result.data);
      setTrouble("");
    } else {
      setTrouble(t("vocab.loadFailed", { reason: result.error }));
    }
  };
  useEffect(() => {
    void load();
    // Read again when another vocabulary is chosen.
  }, [path]);

  const done = async (result: { status: "ok" | "error"; error?: string }) => {
    if (result.status === "error") {
      setTrouble(t("failed", { reason: result.error ?? "" }));
      return false;
    }
    setTrouble("");
    await load();
    onChanged();
    return true;
  };

  const words = list?.words ?? [];
  const typed = query.trim();
  const matching = typed
    ? words.filter(
        (word) =>
          word.term.includes(typed) ||
          word.misheard.some((m) => m.form.includes(typed)),
      )
    : words;
  const shown = typed || all ? matching : matching.slice(0, FIRST_ROWS);
  const exists = words.some((word) => word.term === typed);

  const add = async () => {
    if (!typed || exists || !list?.writable) return;
    const result = await commands.seogiSaveWord(null, typed, []);
    if (await done(result)) setQuery("");
  };

  const onAddKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      void add();
    }
  };

  const when = (day: string) => {
    const [y, m, d] = day.split("-").map(Number);
    if (!y || !m || !d) return "";
    const at = new Date(y, m - 1, d).getTime();
    const ago = daysAgo(at, Date.now());
    if (ago === 0) return t("today");
    if (ago === 1) return t("yesterday");
    return new Intl.DateTimeFormat(i18n.language, {
      month: "long",
      day: "numeric",
    }).format(new Date(at));
  };

  const whence = (word: Word) => {
    const newest = [...word.misheard]
      .filter((m) => m.added_on)
      .sort((a, b) => b.added_on.localeCompare(a.added_on))[0];
    if (!newest) return "";
    const day = when(newest.added_on);
    return newest.meeting
      ? t("vocab.fromMeeting", { when: day, meeting: newest.meeting })
      : t("vocab.addedOn", { when: day });
  };

  return (
    <section aria-label={t("vocab.title")} className="font-medium">
      <p className="m-0 mb-3 text-[13.5px] t3">
        <button type="button" className="lnk t2" onClick={onBack}>
          {t("vocab.back")}
        </button>
        {" › "}
        {t("vocab.title")}
      </p>
      <h2 className="set-h !mt-0">
        {t("vocab.count", { n: words.length.toLocaleString() })}
      </h2>
      <p className="mt-1 mb-4 text-[14px] t2">{t("vocab.lead")}</p>
      {list && !list.writable ? (
        <p className="mb-4 text-[13.5px] t3">{t("vocab.notWritable")}</p>
      ) : null}
      <div className="flex gap-3 items-center mb-2">
        <input
          id="vocab-new"
          type="text"
          className="field flex-1 text-[15px]"
          aria-label={t("vocab.newLabel")}
          placeholder={t("vocab.newPlaceholder")}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={onAddKey}
        />
        {list?.writable ? (
          <button
            type="button"
            className="btn"
            disabled={!typed || exists}
            onClick={() => void add()}
          >
            {t("vocab.add")}
          </button>
        ) : null}
      </div>
      {trouble ? <p className="my-2 text-[13px] rt">{trouble}</p> : null}
      {removed ? (
        <p role="status" className="my-2 text-[13.5px] t2">
          {t("vocab.removed", { term: removed.term })}
          {" · "}
          <button
            type="button"
            className="lnk"
            onClick={async () => {
              const back = removed;
              setRemoved(null);
              await done(
                await commands.seogiSaveWord(null, back.term, back.misheard),
              );
            }}
          >
            {t("vocab.undo")}
          </button>
        </p>
      ) : null}
      <div role="list">
        {shown.map((word) =>
          editing === word.term ? (
            <WordEdit
              key={word.term}
              word={word}
              onCancel={() => setEditing("")}
              onKeep={async (term, misheard) => {
                const result = await commands.seogiSaveWord(
                  word.term,
                  term,
                  misheard,
                );
                if (result.status === "error") return result.error;
                setEditing("");
                await done(result);
                return "";
              }}
            />
          ) : (
            <div key={word.term} role="listitem" className="word-row">
              <div className="flex-1 min-w-0">
                <div className="text-[15px]">{word.term}</div>
                {word.misheard.length ? (
                  <div className="text-[13.5px] t2 mt-0.5">
                    {t("vocab.misheard")}{" "}
                    {word.misheard.map((m) => `「${m.form}」`).join("")}
                    {whence(word) ? ` · ${whence(word)}` : ""}
                  </div>
                ) : word.editable ? (
                  <button
                    type="button"
                    className="lnk t3 text-[13.5px] mt-0.5"
                    onClick={() => setEditing(word.term)}
                  >
                    {t("vocab.writeMisheard")}
                  </button>
                ) : null}
                {word.editable ? null : (
                  <div className="text-[13px] t3 mt-0.5">
                    {t("vocab.readOnly")}
                  </div>
                )}
              </div>
              {word.editable ? (
                <span className="acts">
                  <button
                    type="button"
                    className="lnk"
                    aria-label={`${t("edit.fix")} ${word.term}`}
                    onClick={() => setEditing(word.term)}
                  >
                    {t("edit.fix")}
                  </button>
                  {"·"}
                  <button
                    type="button"
                    className="lnk"
                    aria-label={`${t("edit.remove")} ${word.term}`}
                    onClick={async () => {
                      const result = await commands.seogiRemoveWord(word.term);
                      if (await done(result)) setRemoved(word);
                    }}
                  >
                    {t("edit.remove")}
                  </button>
                </span>
              ) : null}
            </div>
          ),
        )}
      </div>
      {list && words.length === 0 ? (
        <p className="py-3 text-[14px] t3">{t("vocab.empty")}</p>
      ) : null}
      {typed && matching.length === 0 && words.length > 0 ? (
        <p className="py-3 text-[14px] t3">{t("vocab.none")}</p>
      ) : null}
      {!typed && !all && matching.length > FIRST_ROWS ? (
        <p className="pt-3 border-t border-line text-[14px] t2">
          {t("vocab.more", {
            n: (matching.length - FIRST_ROWS).toLocaleString(),
          })}
          {" · "}
          <button type="button" className="lnk" onClick={() => setAll(true)}>
            {t("vocab.showAll")}
          </button>
          {" · "}
          <button
            type="button"
            className="lnk"
            onClick={() => document.getElementById("vocab-new")?.focus()}
          >
            {t("vocab.find")}
          </button>
        </p>
      ) : null}
      <p className="mt-8 text-[13.5px] t3">
        {t("vocab.using")}{" "}
        <span className="t2" title={path}>
          {friendlyPath(path).split(" › ").pop()}
        </span>
        {" · "}
        <button type="button" className="lnk" onClick={() => onPick(true)}>
          {t("vocab.pickFolder")}
        </button>
        {" · "}
        <button type="button" className="lnk" onClick={() => onPick(false)}>
          {t("vocab.pickFile")}
        </button>
      </p>
    </section>
  );
}

/** A word being changed: its name and the ways it is misheard. */
function WordEdit({
  word,
  onCancel,
  onKeep,
}: {
  word: Word;
  onCancel: () => void;
  /** Resolves to an error to show, or "" when kept. */
  onKeep: (term: string, misheard: Misheard[]) => Promise<string>;
}) {
  const { t } = useTranslation(NS);
  const [term, setTerm] = useState(word.term);
  const [forms, setForms] = useState(
    word.misheard.map((m) => m.form).join(", "),
  );
  const [saving, setSaving] = useState(false);
  const [trouble, setTrouble] = useState("");

  const keep = async () => {
    const name = term.trim();
    if (!name || saving) return;
    const listed = formsOf(forms);
    if (listed.some((form) => Array.from(form).length < 2)) {
      setTrouble(t("vocab.misheardHint"));
      return;
    }
    // A way already there keeps when and where it was learnt.
    const misheard: Misheard[] = listed.map(
      (form) =>
        word.misheard.find((m) => m.form === form) ?? {
          form,
          added_on: "",
          meeting: "",
        },
    );
    setSaving(true);
    const error = await onKeep(name, misheard);
    setSaving(false);
    if (error) setTrouble(t("failed", { reason: error }));
  };

  const onKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
    } else if (event.key === "Enter") {
      event.preventDefault();
      void keep();
    }
  };

  return (
    <div role="listitem" className="word-row flex-col !items-stretch">
      <label className="block">
        <span className="block text-[13px] t3 mb-1">
          {t("vocab.termLabel")}
        </span>
        <input
          type="text"
          className="field w-full text-[15px]"
          value={term}
          autoFocus={word.misheard.length > 0}
          onChange={(event) => setTerm(event.target.value)}
          onKeyDown={onKey}
        />
      </label>
      <label className="block">
        <span className="block text-[13px] t3 mb-1">
          {t("vocab.misheardLabel")}
        </span>
        <input
          type="text"
          className="field w-full text-[15px]"
          value={forms}
          autoFocus={word.misheard.length === 0}
          placeholder={t("vocab.misheardPlaceholder")}
          onChange={(event) => setForms(event.target.value)}
          onKeyDown={onKey}
        />
        <span className="block text-[13px] t3 mt-1">
          {t("vocab.misheardHint")}
        </span>
      </label>
      {trouble ? <p className="m-0 text-[13px] rt">{trouble}</p> : null}
      <span className="edit-keys">
        <button type="button" className="lnk t3" onClick={onCancel}>
          {t("edit.cancel")}
        </button>
        <button
          type="button"
          className="lnk"
          disabled={saving || !term.trim()}
          onClick={() => void keep()}
        >
          {t("edit.keep")}
        </button>
      </span>
    </div>
  );
}
