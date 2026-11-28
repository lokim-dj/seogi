// The minutes' tasks: one group of them, and adding or taking one out.

import { Fragment, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type MinutesSection } from "@/bindings";
import { TimeKey, useSpeakerName } from "../../components/common";
import { Marked } from "../../components/Found";
import { has } from "../../lib/found";
import { EditedMark, TextEdit, WasWords } from "../../components/Edit";
import { NS } from "../../i18n";
import { taskLine } from "../../lib/tasks";
import { quoteOf } from "./Details";

export function TaskGroup({
  meetingId,
  section,
  showTitle,
  people,
  found,
  words,
  onListen,
  onDone,
  onChanged,
}: {
  meetingId: string;
  section: MinutesSection;
  showTitle: boolean;
  /** The people of the meeting, whose names a line may leave out. */
  people: string[];
  /** The task a search went to, by its place in the section. */
  found: number | null;
  words: string;
  onListen: (ms: number) => void;
  onDone: (index: number, done: boolean) => void;
  onChanged: () => Promise<void>;
}) {
  const { t } = useTranslation(NS);
  const speakerName = useSpeakerName();
  // The task being changed, by its place; "add" while one is being added.
  const [editing, setEditing] = useState<number | "add" | null>(null);
  // The changed task whose words as written are shown.
  const [seeing, setSeeing] = useState<number | null>(null);
  const [showRemoved, setShowRemoved] = useState(false);
  const [trouble, setTrouble] = useState("");
  const removed = section.removed ?? [];

  /** Asks the backend for a change and reads the meeting again; the error
   *  to show, or "" when it went through. */
  const change = async (
    work: Promise<{ status: "ok" } | { status: "error"; error: string }>,
  ): Promise<string> => {
    const result = await work;
    if (result.status === "error") return result.error;
    setEditing(null);
    setSeeing(null);
    await onChanged();
    return "";
  };
  const act = async (
    work: Promise<{ status: "ok" } | { status: "error"; error: string }>,
  ) => {
    const error = await change(work);
    setTrouble(error ? t("failed", { reason: error }) : "");
  };

  return (
    <div className="mb-2">
      {showTitle ? (
        <h3 className="m-0 mt-2 mb-0.5 text-[13px] font-normal t3">
          {section.title}
        </h3>
      ) : null}
      {section.entries.map((entry, index) => {
        const source = entry.sources[0];
        const { who, text, unowned } = taskLine(section, entry, people);
        const owner = who ? speakerName(who) : "";
        const here = found === index;
        const original = entry.original ?? "";
        return (
          <Fragment key={`${section.key}-${index}`}>
            <div
              className={`task ${entry.faint ? "opacity-70" : ""}`}
              data-found={here ? "" : undefined}
              data-task={index}
            >
              <input
                type="checkbox"
                className="chk"
                checked={entry.done ?? false}
                onChange={() => onDone(index, !(entry.done ?? false))}
                aria-label={t("home.doneLabel", { task: entry.text })}
              />
              {editing === index ? (
                // The words and the due column, so there is room to write.
                <span className="flex items-center gap-2 min-w-0 col-span-2">
                  {owner ? <span className="who shrink-0">{owner}</span> : null}
                  <TextEdit
                    initial={text}
                    label={t("edit.taskLabel")}
                    onCancel={() => setEditing(null)}
                    onKeep={(words) =>
                      change(
                        commands.seogiEditTask(
                          meetingId,
                          section.key,
                          index,
                          words,
                        ),
                      )
                    }
                  />
                </span>
              ) : (
                <span>
                  <span
                    className={entry.done ? "t3 line-through" : undefined}
                    title={quoteOf(entry) ? `「${quoteOf(entry)}」` : undefined}
                  >
                    {owner ? (
                      <span className="who">
                        {here ? <Marked text={owner} words={words} /> : owner}
                      </span>
                    ) : null}
                    {here ? (
                      <Marked
                        text={text}
                        words={words}
                        wholeIfAbsent={!has(owner, words)}
                      />
                    ) : (
                      text
                    )}
                  </span>
                  {unowned ? (
                    <span className="unowned">{t("ownerUnknown")}</span>
                  ) : null}
                  {(entry.reasons ?? []).length > 0 ? (
                    <em>{t("minutes.checkMark")}</em>
                  ) : null}
                  {original ? (
                    <EditedMark
                      open={seeing === index}
                      onToggle={() =>
                        setSeeing(seeing === index ? null : index)
                      }
                    />
                  ) : null}
                  <span className="acts">
                    <button
                      type="button"
                      className="lnk"
                      onClick={() => {
                        setEditing(index);
                        setTrouble("");
                      }}
                    >
                      {t("edit.fix")}
                    </button>
                    <span aria-hidden="true">·</span>
                    <button
                      type="button"
                      className="lnk"
                      onClick={() =>
                        void act(
                          commands.seogiRemoveTask(
                            meetingId,
                            section.key,
                            index,
                          ),
                        )
                      }
                    >
                      {t("edit.remove")}
                    </button>
                  </span>
                </span>
              )}
              {editing === index ? null : (
                <span className={`due ${entry.due ? "" : "none"}`}>
                  {entry.due || t("dueUnknown")}
                </span>
              )}
              {source ? (
                <TimeKey ms={source.ms} onListen={onListen} />
              ) : (
                <span />
              )}
            </div>
            {seeing === index && original ? <WasWords text={original} /> : null}
          </Fragment>
        );
      })}
      <TaskFoot
        adding={editing === "add"}
        removed={removed.length}
        open={showRemoved}
        onAdd={() => {
          setEditing("add");
          setTrouble("");
        }}
        onCancelAdd={() => setEditing(null)}
        onKeepAdd={(words) =>
          change(commands.seogiAddTask(meetingId, section.key, words))
        }
        onRemoved={() => {
          // One taken out comes straight back; with more, pick which.
          if (removed.length === 1) {
            void act(commands.seogiRestoreTask(meetingId, section.key, 0));
          } else setShowRemoved(!showRemoved);
        }}
      />
      {showRemoved && removed.length > 1
        ? removed.map((gone, index) => (
            <div key={`gone-${index}`} className="task-gone">
              <span className="t3 line-through">{gone.entry.text}</span>
              <button
                type="button"
                className="lnk"
                onClick={() =>
                  void act(
                    commands.seogiRestoreTask(meetingId, section.key, index),
                  )
                }
              >
                {t("edit.restore")}
              </button>
            </div>
          ))
        : null}
      {trouble ? <p className="m-0 mt-1 text-[13px] rt">{trouble}</p> : null}
    </div>
  );
}

/** "+ 할 일 더하기" where the minutes found no task. */
export function NoTasksAdd({
  meetingId,
  section,
  onChanged,
}: {
  meetingId: string;
  /** The task section, or "" to have the template's first one made. */
  section: string;
  onChanged: () => Promise<void>;
}) {
  const [adding, setAdding] = useState(false);
  return (
    <TaskFoot
      adding={adding}
      removed={0}
      open={false}
      onAdd={() => setAdding(true)}
      onCancelAdd={() => setAdding(false)}
      onKeepAdd={async (words) => {
        const result = await commands.seogiAddTask(meetingId, section, words);
        if (result.status === "error") return result.error;
        setAdding(false);
        await onChanged();
        return "";
      }}
    />
  );
}

/** Under a list of tasks: "+ 할 일 더하기", and the ones taken out. */
function TaskFoot({
  adding,
  removed,
  open,
  onAdd,
  onCancelAdd,
  onKeepAdd,
  onRemoved,
}: {
  adding: boolean;
  removed: number;
  open: boolean;
  onAdd: () => void;
  onCancelAdd: () => void;
  onKeepAdd: (text: string) => Promise<string>;
  onRemoved?: () => void;
}) {
  const { t } = useTranslation(NS);
  return (
    <div className="task-foot">
      {adding ? (
        <TextEdit
          initial=""
          label={t("edit.addLabel")}
          placeholder={t("edit.addPlaceholder")}
          onCancel={onCancelAdd}
          onKeep={onKeepAdd}
        />
      ) : (
        <button type="button" className="quiet" onClick={onAdd}>
          {t("edit.add")}
        </button>
      )}
      {removed > 0 && onRemoved ? (
        <button
          type="button"
          className="quiet ml-auto"
          aria-expanded={removed > 1 ? open : undefined}
          onClick={onRemoved}
        >
          {t("edit.removed", { n: removed })}
          {" · "}
          <span className="underline underline-offset-[3px] decoration-[var(--t3)]">
            {t("edit.restore")}
          </span>
        </button>
      ) : null}
    </div>
  );
}
