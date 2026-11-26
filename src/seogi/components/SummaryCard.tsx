// The one-page summary ("한 장 요약"): the minutes on one picture to send
// in a chat (4:5, 1080×1350) or to print (A4), drawn as the chosen design A
// "신문 1면". Saved into the meeting's own folder without asking where; the
// original recording is never touched.

import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type CheckReason,
  type Meeting,
  type TalkStats,
} from "@/bindings";
import { NS } from "../i18n";
import {
  cardOf,
  fitSteps,
  type Card,
  type CardFormat,
  type CardLine,
} from "../lib/card";
import { clock, hm } from "../lib/format";
import { A4_POINTS, jpegPdf } from "../lib/pdf";
import { useLength, useSpeakerName } from "./common";
import { Mark } from "./Mark";
import gmarketMedium from "../fonts/GmarketSansMedium.otf?url";
import gmarketBold from "../fonts/GmarketSansBold.otf?url";

export type CardTheme = "dark" | "light";

/** The width the card is drawn on; every size is in its units. */
const UNITS = 540;
/** Height over width of each shape. */
const RATIO: Record<CardFormat, number> = { kakao: 5 / 4, a4: 297 / 210 };
/** How much larger than the drawing the saved picture is: 1080 pixels wide
 *  for the phone picture, 200 dots an inch for the A4 page. */
const SCALE: Record<CardFormat, number> = { kakao: 2, a4: 1654 / UNITS };

/** The name the card is saved under in the meeting's folder. */
export const CARD_FILE = "한 장 요약";

/** "11월 20일 (금)", the day of the masthead. */
function dayOf(ms: number, lang: string): string {
  const d = new Date(ms);
  const day = new Intl.DateTimeFormat(lang, {
    month: "long",
    day: "numeric",
  }).format(d);
  const weekday = new Intl.DateTimeFormat(lang, { weekday: "short" }).format(d);
  return `${day} (${weekday})`;
}

function Time({ line }: { line: CardLine }) {
  if (line.ms === null) return <span />;
  return (
    <span className={`sc-k${line.doubted ? " sc-red" : ""}`}>
      {clock(line.ms)}
    </span>
  );
}

function Lines({ lines, more }: { lines: CardLine[]; more: number }) {
  const { t } = useTranslation(NS);
  return (
    <ol>
      {lines.map((line, index) => (
        <li key={index}>
          <span>
            <span className="sc-tx">{line.text}</span>
            {line.flagged ? (
              <span className="sc-who">
                <span className="sc-red">{t("card.toCheck")}</span>
                {line.note ? ` · ${line.note}` : ""}
              </span>
            ) : line.note ? (
              <span className="sc-who">{line.note}</span>
            ) : null}
          </span>
          <Time line={line} />
        </li>
      ))}
      {more > 0 ? (
        <li>
          <span className="sc-more">{t("card.more", { n: more })}</span>
        </li>
      ) : null}
    </ol>
  );
}

/** A speaker's mark in their tone. */
function Glyph({ tone }: { tone: number | null }) {
  if (tone === null) return null;
  return (
    <span style={{ color: `var(--sp${tone})` }}>
      <Mark tight color="currentColor" className="sc-gl" />{" "}
    </span>
  );
}

/** The card drawn from `card`, `width` pixels wide. */
function CardDrawing({
  card,
  format,
  theme,
  width,
  drawing,
}: {
  card: Card;
  format: CardFormat;
  theme: CardTheme;
  width: number;
  drawing?: React.Ref<HTMLElement>;
}) {
  const { t, i18n } = useTranslation(NS);
  const length = useLength();
  const height = Math.round(width * RATIO[format]);
  const style = {
    width,
    height,
    "--u": `${width / UNITS}px`,
  } as React.CSSProperties;

  const meta = [
    `${dayOf(card.startedAtMs, i18n.language)} ${hm(card.startedAtMs)}`,
    card.durationMs > 0 ? length(card.durationMs) : "",
    card.kind ?? "",
  ].filter(Boolean);

  const { decisions, numbers, checks, flow } = card;
  const first = decisions || numbers;
  const last = card.share.length > 0 || checks;

  return (
    <article
      ref={drawing}
      className={`scard${theme === "light" ? " sc-light" : ""}`}
      style={style}
      data-card={format}
    >
      <header className="sc-mast">
        <div className="sc-meta">{meta.join(" · ")}</div>
        <div className="min-w-0">
          <div className="sc-head">{card.title}</div>
          {card.sub ? <div className="sc-sub">{card.sub}</div> : null}
          {card.people.length > 0 ? (
            <div className="sc-ppl">
              {card.people.map((person, index) => (
                <span key={index}>
                  <Glyph tone={person.tone} />
                  {person.name}
                </span>
              ))}
            </div>
          ) : null}
        </div>
        <div className={`sc-flag${card.checkCount === 0 ? " sc-none" : ""}`}>
          <b>{card.checkCount}</b>
          {t("card.toCheck")}
        </div>
      </header>

      {first ? (
        <div className={`sc-row${decisions && numbers ? " sc-two" : ""}`}>
          {decisions ? (
            <div>
              <div className="sc-lab">
                {decisions.title}
                <em>{decisions.lines.length + decisions.more}</em>
              </div>
              <Lines lines={decisions.lines} more={decisions.more} />
            </div>
          ) : null}
          {numbers ? (
            <div>
              <div className="sc-lab">
                {t("card.numbers")}
                {numbers.lead ? <Time line={numbers.lead.line} /> : null}
              </div>
              {numbers.lead ? (
                <>
                  <div className="sc-big">
                    {numbers.lead.value}
                    <small>{numbers.lead.unit}</small>
                  </div>
                  <div className="sc-delta">{numbers.lead.line.text}</div>
                </>
              ) : null}
              {numbers.lines.length > 0 || numbers.more > 0 ? (
                <div className="sc-dims">
                  <Lines lines={numbers.lines} more={numbers.more} />
                </div>
              ) : null}
            </div>
          ) : null}
        </div>
      ) : null}

      {card.tasks.length > 0 ? (
        <div
          className={`sc-row${card.tasks.length > 1 ? " sc-two sc-even" : ""}`}
        >
          {card.tasks.map((group) => (
            <div key={group.title}>
              <div className="sc-lab">
                {group.title}
                <em>{group.lines.length + group.more}</em>
              </div>
              <Lines lines={group.lines} more={group.more} />
            </div>
          ))}
        </div>
      ) : null}

      {flow ? (
        <div className={`sc-flow${last ? "" : " sc-last"}`}>
          <div className="sc-lab">
            {t("card.flow")}
            <em>{length(flow.totalMs)}</em>
          </div>
          <div className="sc-band" role="img" aria-label={t("card.flow")}>
            {flow.ticks.map((tick, index) => (
              <i
                key={`t${index}`}
                className={`sc-tick${tick.red ? " sc-red" : ""}`}
                style={{ left: `${(tick.ms / flow.totalMs) * 100}%` }}
              />
            ))}
            {flow.parts[0].startMs > 0 ? (
              <i
                className="sc-bar"
                style={{
                  left: 0,
                  width: `calc(${(flow.parts[0].startMs / flow.totalMs) * 100}% - var(--u) * 2)`,
                  background: "var(--s2)",
                }}
              />
            ) : null}
            {flow.parts.map((part, index) => {
              const left = (part.startMs / flow.totalMs) * 100;
              const span = ((part.endMs - part.startMs) / flow.totalMs) * 100;
              return (
                <Fragment key={`p${index}`}>
                  <i
                    className="sc-bar"
                    style={{
                      left: `${left}%`,
                      width: `calc(${span}% - var(--u) * 2)`,
                      background: index % 2 === 0 ? "var(--t2)" : "var(--t3)",
                    }}
                  />
                  <i
                    className="sc-part"
                    style={{ left: `${left}%`, width: `${span}%` }}
                  >
                    {part.title}
                    <small>{clock(part.startMs)}</small>
                  </i>
                </Fragment>
              );
            })}
          </div>
        </div>
      ) : null}

      {last ? (
        <div
          className={`sc-row sc-last${card.share.length > 0 && checks ? " sc-two sc-wide-right" : ""}`}
        >
          {card.share.length > 0 ? (
            <div>
              <div className="sc-lab">{t("card.share")}</div>
              <div className="sc-share">
                {card.share.map((piece, index) => (
                  <i
                    key={index}
                    style={{
                      flex: Math.max(1, piece.percent),
                      background: `var(--sp${piece.tone})`,
                    }}
                  />
                ))}
              </div>
              <div className="sc-who4">
                {card.share.map((piece, index) => (
                  <div key={index}>
                    <Glyph tone={piece.tone} />
                    <span>{piece.name}</span> {piece.percent}
                  </div>
                ))}
              </div>
            </div>
          ) : null}
          {checks ? (
            <div>
              <div className="sc-lab">
                <span className="sc-red">{t("card.toCheck")}</span>
                <em>{checks.lines.length + checks.more}</em>
              </div>
              <div className="sc-chk">
                <Lines lines={checks.lines} more={checks.more} />
              </div>
            </div>
          ) : null}
        </div>
      ) : null}

      <footer className="sc-foot">
        <span>{t("card.foot")}</span>
        <span className="sc-brand">
          <Mark color="currentColor" />
          {t("brand")}
        </span>
      </footer>
    </article>
  );
}

/**
 * The card of `meeting`, `width` pixels wide. Long lines may not fit the
 * page: then each part takes fewer lines, a step at a time, until the
 * drawing fits; `onFit` hears the step it settled on. With `fit` given, it
 * is drawn at that step (the preview copies the saved drawing's).
 */
export function SummaryCardView({
  meeting,
  talk,
  format,
  theme,
  width,
  fit: fixed,
  onFit,
}: {
  meeting: Meeting;
  talk: TalkStats | null;
  format: CardFormat;
  theme: CardTheme;
  width: number;
  fit?: number;
  onFit?: (fit: number) => void;
}) {
  const words = useCardWords();
  const drawing = useRef<HTMLElement | null>(null);
  const [fontsReady, setFontsReady] = useState(
    () => document.fonts?.status === "loaded",
  );
  // The step for these inputs; new inputs start again from the most lines.
  const [tried, setTried] = useState({
    meeting,
    talk,
    format,
    fontsReady,
    fit: 0,
  });
  const same =
    tried.meeting === meeting &&
    tried.talk === talk &&
    tried.format === format &&
    tried.fontsReady === fontsReady;
  const fit = fixed ?? (same ? tried.fit : 0);

  useEffect(() => {
    let on = true;
    void document.fonts?.ready.then(() => on && setFontsReady(true));
    return () => {
      on = false;
    };
  }, []);

  useLayoutEffect(() => {
    if (fixed !== undefined) return;
    const node = drawing.current;
    if (!node) return;
    if (
      node.scrollHeight > node.clientHeight + 1 &&
      fit < fitSteps(format) - 1
    ) {
      setTried({ meeting, talk, format, fontsReady, fit: fit + 1 });
    } else {
      onFit?.(fit);
    }
  });

  return (
    <CardDrawing
      card={cardOf(meeting, talk, format, words, fit)}
      format={format}
      theme={theme}
      width={width}
      drawing={drawing}
    />
  );
}

/** The card's words, from the screens' texts. */
export function useCardWords() {
  const { t } = useTranslation(NS);
  const speakerName = useSpeakerName();
  return {
    untitled: t("untitled"),
    customer: (name: string) => t("card.customer", { name }),
    speaker: speakerName,
    reason: (reason: CheckReason) =>
      t(`minutes.reason.${reason}`, {
        defaultValue: t("minutes.reason.weak_support"),
      }),
  };
}

/** The bundled Gmarket Sans for the picture, so the saved file is set in
 *  the same type as the screen (and only it: no other font is fetched). A
 *  picture cannot load files, so the fonts go in as data; read once. */
let fontCss: Promise<string> | null = null;
function fontsAsData(): Promise<string> {
  fontCss ??= Promise.all(
    (
      [
        [gmarketMedium, 500],
        [gmarketBold, 700],
      ] as const
    ).map(async ([src, weight]) => {
      const response = await fetch(new URL(src, document.baseURI));
      if (!response.ok) throw new Error(`font ${response.status}`);
      const bytes = new Uint8Array(await response.arrayBuffer());
      return `@font-face{font-family:"Gmarket Sans";src:url(data:font/otf;base64,${base64(bytes)}) format("opentype");font-weight:${weight};font-style:normal;}`;
    }),
  )
    .then((faces) => faces.join("\n"))
    .catch((error: unknown) => {
      fontCss = null;
      throw error;
    });
  return fontCss;
}

/** The card drawn on a canvas at the saved size. */
async function draw(
  node: HTMLElement,
  format: CardFormat,
): Promise<HTMLCanvasElement> {
  await document.fonts.ready;
  const [{ domToCanvas }, cssText] = await Promise.all([
    import("modern-screenshot"),
    fontsAsData(),
  ]);
  return domToCanvas(node, {
    scale: SCALE[format],
    font: { cssText },
  });
}

function blobOf(
  canvas: HTMLCanvasElement,
  type: string,
  quality?: number,
): Promise<Blob> {
  return new Promise((resolve, reject) =>
    canvas.toBlob(
      (blob) => (blob ? resolve(blob) : reject(new Error("no picture"))),
      type,
      quality,
    ),
  );
}

function base64(bytes: Uint8Array): string {
  let text = "";
  for (let at = 0; at < bytes.length; at += 0x8000) {
    text += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  }
  return btoa(text);
}

/** The phone picture as PNG bytes. */
async function pngOf(node: HTMLElement): Promise<Uint8Array> {
  const canvas = await draw(node, "kakao");
  return new Uint8Array(
    await (await blobOf(canvas, "image/png")).arrayBuffer(),
  );
}

/** The A4 page as a PDF holding the picture. */
async function pdfOf(node: HTMLElement): Promise<Uint8Array> {
  const canvas = await draw(node, "a4");
  const jpeg = new Uint8Array(
    await (await blobOf(canvas, "image/jpeg", 0.92)).arrayBuffer(),
  );
  return jpegPdf(jpeg, canvas.width, canvas.height, A4_POINTS);
}

type Done =
  | { what: "saved"; kind: "png" | "pdf" }
  | { what: "copied" }
  | { what: "failed" };

/** The card shown over the minutes, with saving and copying. */
export function SummaryCardDialog({
  meeting,
  onClose,
}: {
  meeting: Meeting;
  onClose: () => void;
}) {
  const { t } = useTranslation(NS);
  const [talk, setTalk] = useState<TalkStats | null>(null);
  const [format, setFormat] = useState<CardFormat>("kakao");
  const [theme, setTheme] = useState<CardTheme>(() =>
    window.matchMedia?.("(prefers-color-scheme: light)").matches
      ? "light"
      : "dark",
  );
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<Done | null>(null);
  const kakaoNode = useRef<HTMLDivElement | null>(null);
  const a4Node = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!meeting.transcript) return;
    void commands.seogiTalk(meeting.id).then((result) => {
      if (result.status === "ok") setTalk(result.data);
    });
  }, [meeting.id, meeting.transcript]);

  useEffect(() => {
    const away = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", away);
    return () => window.removeEventListener("keydown", away);
  }, [onClose]);

  const [fits, setFits] = useState<Record<CardFormat, number>>({
    kakao: 0,
    a4: 0,
  });
  const fitOf = (shape: CardFormat) => (fit: number) =>
    setFits((now) => (now[shape] === fit ? now : { ...now, [shape]: fit }));

  const run = async (work: () => Promise<Done>) => {
    setBusy(true);
    setDone(null);
    try {
      setDone(await work());
    } catch (error) {
      console.error("summary card:", error);
      setDone({ what: "failed" });
    } finally {
      setBusy(false);
    }
  };

  const save = (kind: "png" | "pdf") =>
    run(async () => {
      setFormat(kind === "png" ? "kakao" : "a4");
      const node = (kind === "png" ? kakaoNode : a4Node).current
        ?.firstElementChild;
      if (!(node instanceof HTMLElement)) throw new Error("no card");
      const bytes = kind === "png" ? await pngOf(node) : await pdfOf(node);
      const result = await commands.seogiSaveSummaryCard(
        meeting.id,
        kind,
        base64(bytes),
      );
      if (result.status === "error") throw new Error(result.error);
      return { what: "saved", kind };
    });

  const copy = () =>
    run(async () => {
      const node = kakaoNode.current?.firstElementChild;
      if (!(node instanceof HTMLElement)) throw new Error("no card");
      const result = await commands.seogiCopyImage(base64(await pngOf(node)));
      if (result.status === "error") throw new Error(result.error);
      return { what: "copied" };
    });

  const previewWidth = format === "kakao" ? 400 : 360;
  const toggle = (on: boolean) => (on ? "true" : "false");

  return (
    <div
      className="fixed inset-0 z-30 grid place-items-center overflow-auto bg-[var(--sh)] px-4 py-6"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("card.title")}
        className="w-fit max-w-full p-5 rounded-[12px] bg-s1 shadow-[0_12px_32px_var(--sh)]"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-2 mb-3">
          <p className="m-0 text-[15.5px]">{t("card.title")}</p>
          <div className="flex flex-wrap gap-3 text-[13px]">
            <span className="seg" role="group" aria-label={t("card.shape")}>
              <button
                type="button"
                aria-pressed={toggle(format === "kakao")}
                onClick={() => setFormat("kakao")}
              >
                {t("card.kakao")}
              </button>
              <button
                type="button"
                aria-pressed={toggle(format === "a4")}
                onClick={() => setFormat("a4")}
              >
                {"A4"}
              </button>
            </span>
            <span className="seg" role="group" aria-label={t("card.paper")}>
              <button
                type="button"
                aria-pressed={toggle(theme === "dark")}
                onClick={() => setTheme("dark")}
              >
                {t("card.dark")}
              </button>
              <button
                type="button"
                aria-pressed={toggle(theme === "light")}
                onClick={() => setTheme("light")}
              >
                {t("card.light")}
              </button>
            </span>
          </div>
        </div>

        <div
          className="mx-auto max-w-full overflow-hidden shadow-[0_0_0_1px_var(--line)]"
          style={{ width: previewWidth }}
        >
          <SummaryCardView
            meeting={meeting}
            talk={talk}
            format={format}
            theme={theme}
            width={previewWidth}
            fit={fits[format]}
          />
        </div>

        <p className="mt-3 mb-0 max-w-[400px] text-[13px] t3">
          {t("card.lead")}
        </p>
        <p
          role="status"
          className={`mt-2 mb-0 min-h-[20px] text-[13.5px] ${done?.what === "failed" ? "rt" : "t2"}`}
        >
          {busy
            ? t("card.making")
            : done?.what === "saved"
              ? t("card.saved", { name: `${CARD_FILE}.${done.kind}` })
              : done?.what === "copied"
                ? t("card.copied")
                : done?.what === "failed"
                  ? t("card.failed")
                  : ""}
          {done?.what === "saved" ? (
            <>
              {" "}
              <button
                type="button"
                className="lnk"
                onClick={() =>
                  void commands.seogiReveal(meeting.id, `card_${done.kind}`)
                }
              >
                {t("card.showInFolder")}
              </button>
            </>
          ) : null}
        </p>
        <div className="flex flex-wrap justify-end gap-2 mt-4">
          <button type="button" className="btn" onClick={onClose}>
            {t("card.close")}
          </button>
          <button
            type="button"
            className="btn"
            disabled={busy}
            onClick={() => void copy()}
          >
            {t("card.copy")}
          </button>
          <button
            type="button"
            className="btn"
            disabled={busy}
            onClick={() => void save("pdf")}
          >
            {t("card.savePdf")}
          </button>
          <button
            type="button"
            className="btn solid"
            autoFocus
            disabled={busy}
            onClick={() => void save("png")}
          >
            {t("card.savePng")}
          </button>
        </div>
      </div>

      {/* The cards at their drawing size, out of sight, for saving. */}
      <div
        aria-hidden="true"
        style={{
          position: "fixed",
          left: -10000,
          top: 0,
          pointerEvents: "none",
        }}
      >
        <div ref={kakaoNode}>
          <SummaryCardView
            meeting={meeting}
            talk={talk}
            format="kakao"
            theme={theme}
            width={UNITS}
            onFit={fitOf("kakao")}
          />
        </div>
        <div ref={a4Node}>
          <SummaryCardView
            meeting={meeting}
            talk={talk}
            format="a4"
            theme={theme}
            width={UNITS}
            onFit={fitOf("a4")}
          />
        </div>
      </div>
    </div>
  );
}
