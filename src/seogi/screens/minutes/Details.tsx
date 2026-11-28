// What is folded under "자세히": ruled lists of entries, and questions put
// to the recording.

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type Answer, type Entry } from "@/bindings";
import { TimeKey } from "../../components/common";
import { Marked } from "../../components/Found";
import { NS } from "../../i18n";
import { answer } from "../../lib/naming";
import { bare } from "./Parts";

/** The words an entry rests on, unless they only repeat the entry. */
export function quoteOf(entry: Entry): string {
  const quote = entry.sources[0]?.quote ?? "";
  return quote && bare(quote) !== bare(entry.text) ? quote : "";
}

export function RuledList({
  entries,
  found,
  words,
  onListen,
}: {
  entries: Entry[];
  /** The line a search went to, by its place in the list. */
  found: number | null;
  words: string;
  onListen: (ms: number) => void;
}) {
  const { t } = useTranslation(NS);
  return (
    <ul className="m-0 p-0 list-none">
      {entries.map((entry, index) => {
        const quote = quoteOf(entry);
        const source = entry.sources[0];
        const here = found === index;
        return (
          <li
            key={index}
            data-found={here ? "" : undefined}
            className={`grid grid-cols-[minmax(0,1fr)_auto] gap-3 py-2 border-t border-line first:border-t-0 ${
              entry.faint ? "opacity-70" : ""
            }`}
          >
            <span>
              {here ? (
                <Marked text={entry.text} words={words} wholeIfAbsent />
              ) : (
                entry.text
              )}
              {(entry.reasons ?? []).length > 0 ? (
                <em className="not-italic rt text-[13px] ml-2">
                  {t("minutes.checkMark")}
                </em>
              ) : null}
              {quote ? (
                <span className="block text-[13px] t3">{`「${quote}」`}</span>
              ) : null}
            </span>
            {source ? (
              <TimeKey
                ms={source.ms}
                onListen={onListen}
                className="self-start"
              />
            ) : (
              <span />
            )}
          </li>
        );
      })}
    </ul>
  );
}

interface Asked {
  question: string;
  answer: Answer | null;
  trouble: string;
}

/** Questions put to the recording. Each answer shows the words it rests on;
 *  one whose words are not in the transcript says so. */
export function Ask({
  id,
  onListen,
}: {
  id: string;
  onListen: (ms: number) => void;
}) {
  const { t } = useTranslation(NS);
  const [question, setQuestion] = useState("");
  const [asked, setAsked] = useState<Asked[]>([]);
  const waiting = asked.some((item) => !item.answer && !item.trouble);

  const ask = async () => {
    const words = question.trim();
    if (!words || waiting) return;
    setQuestion("");
    setAsked((list) => [
      { question: words, answer: null, trouble: "" },
      ...list,
    ]);
    const result = await commands.seogiAsk(id, words);
    setAsked((list) =>
      list.map((item, index) =>
        index === 0
          ? result.status === "ok"
            ? { ...item, answer: result.data }
            : { ...item, trouble: result.error }
          : item,
      ),
    );
  };

  return (
    <div>
      <p className="mt-0 mb-2.5 text-[13.5px] text-dim">
        {t("minutes.askLead")}
      </p>
      <form
        className="flex gap-2 pb-3"
        onSubmit={(event) => {
          event.preventDefault();
          void ask();
        }}
      >
        <input
          type="text"
          value={question}
          onChange={(event) => setQuestion(event.target.value)}
          placeholder={t("minutes.askPlaceholder")}
          aria-label={t("minutes.ask")}
          className="field field-soft flex-1 min-w-0"
        />
        <button
          type="submit"
          className="key key-small"
          disabled={waiting || !question.trim()}
        >
          {t("minutes.askButton")}
        </button>
      </form>
      {asked.map((item, index) => (
        <div
          key={asked.length - index}
          className={`py-2.5 ${
            index === 0 ? "border-t border-line" : "border-t border-line"
          }`}
        >
          <strong className="text-[14px]">{item.question}</strong>
          {item.trouble ? (
            <p
              role="alert"
              className="must-check mt-1.5 mb-0 px-2 py-1 font-normal"
            >
              {t("failed", { reason: item.trouble })}
            </p>
          ) : !item.answer ? (
            <p className="mt-1.5 mb-0 text-[13.5px] text-dim">
              {t("minutes.asking")}
            </p>
          ) : (
            <>
              <p
                className={`mt-1.5 mb-0 text-[14px] leading-[1.6] ${
                  item.answer.found ? "" : "text-dim"
                }`}
              >
                {item.answer.text}
              </p>
              {item.answer.unsupported ? (
                <p className="must-check mt-1.5 mb-0 px-2 py-1 text-[13.5px] font-normal">
                  {t("minutes.askUnsupported")}
                </p>
              ) : null}
              {item.answer.sources.map((source) => (
                <div key={source.ms} className="flex items-start gap-2 mt-1.5">
                  <TimeKey
                    ms={source.ms}
                    onListen={onListen}
                    icon={false}
                    className="!h-7 !px-[7px] flex-none"
                  />
                  <span className="pt-0.5 text-[13.5px] text-dim">{`「${source.quote}」`}</span>
                </div>
              ))}
            </>
          )}
        </div>
      ))}
    </div>
  );
}
