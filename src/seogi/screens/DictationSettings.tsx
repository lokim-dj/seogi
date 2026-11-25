// Settings › 받아쓰기: dictation, Seogi's second job. The same rows as the
// rest of the settings: a name, a line saying what it does, its value. Each
// change is saved as it is made. Everything here comes from the backend
// (seogiDictation, seogiDictationStatus); nothing is decided on the screen
// except which keys are offered.

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import {
  commands,
  type Dictation,
  type DictationStatus,
  type Press,
  type Provider,
  type Speed,
  type Typing,
} from "@/bindings";
import { NS } from "../i18n";
import { keysShort } from "../lib/format";
import { useSeogi } from "../store";

const PRESSES: Press[] = ["both", "hold", "toggle"];
const TYPINGS: Typing[] = ["paste", "type", "copy"];

/** The keys offered for the shortcut, the default first. A key picked from
 *  a list cannot be mistyped; anything else set earlier stays listed. */
const KEYS_MAC = ["option+space", "ctrl+option+space", "ctrl+shift+space"];
const KEYS_OTHER = ["alt+space", "ctrl+alt+space", "ctrl+shift+space"];

/** Keys in one form, so `Ctrl+Alt+R` and `option+ctrl+r` compare equal. */
function keySet(keys: string): string {
  return keys
    .toLowerCase()
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) =>
      part === "option" || part === "opt"
        ? "alt"
        : part === "control"
          ? "ctrl"
          : ["command", "meta", "super", "win"].includes(part)
            ? "cmd"
            : part,
    )
    .sort()
    .join("+");
}

function keyLabel(keys: string, mac: boolean): string {
  return keysShort(keys, mac).replace(/space/i, "Space");
}

/** One way of tidying, as the list names it. */
function providerKey(provider: Provider): string {
  return JSON.stringify(provider);
}

export default function DictationSettings({
  onNote,
}: {
  onNote: (note: string) => void;
}) {
  const { t } = useTranslation(NS);
  const downloads = useSeogi((s) => s.downloads);
  const [dictation, setDictation] = useState<Dictation | null>(null);
  const [status, setStatus] = useState<DictationStatus | null>(null);
  // "모두 지우기" asks once more, in its place.
  const [askingAll, setAskingAll] = useState(false);
  const mac = platform() === "macos";

  const look = useCallback(async () => {
    const result = await commands.seogiDictationStatus();
    if (result.status === "ok") setStatus(result.data);
  }, []);

  useEffect(() => {
    void commands.seogiDictation().then(setDictation);
    void look();
  }, [look]);

  // Waiting for the user to allow typing in System Settings: look again
  // every couple of seconds, so the line changes by itself.
  const waiting = !!dictation?.enabled && status?.permitted === false;
  useEffect(() => {
    if (!waiting) return;
    const timer = window.setInterval(() => void look(), 2000);
    return () => window.clearInterval(timer);
  }, [waiting, look]);

  // A model being downloaded is chosen once it is here.
  const downloading = Object.keys(downloads).length;
  useEffect(() => {
    if (downloading === 0) void look();
  }, [downloading, look]);

  if (!dictation || !status) return null;

  const save = async (next: Dictation) => {
    const before = dictation;
    setDictation(next);
    const result = await commands.seogiSetDictation(next);
    if (result.status === "error") {
      setDictation(before);
      const [code, by] = result.error.split(":");
      onNote(
        t(`dictation.errors.${code}`, {
          by: by ? t(`dictation.shortcutTaken.${by}`) : "",
          defaultValue: t("failed", { reason: result.error }),
        }),
      );
      return;
    }
    onNote(t("settings.saved"));
    void look();
  };

  // Gone from the list at once, and from the dictation history for good.
  const forget = async (ids: number[] | null) => {
    setAskingAll(false);
    setStatus({
      ...status,
      recent: ids ? status.recent.filter((item) => !ids.includes(item.id)) : [],
    });
    const result = await commands.seogiForgetDictations(ids);
    onNote(
      result.status === "error"
        ? t("failed", { reason: result.error })
        : t("dictation.forgotten"),
    );
    // Read again either way: a failure halfway shows what is really left.
    void look();
  };

  const takenBy = (keys: string) =>
    status.taken.find((taken) => keySet(taken.keys) === keySet(keys))?.by;

  const offered = mac ? KEYS_MAC : KEYS_OTHER;
  const keys = offered.some((k) => keySet(k) === keySet(dictation.shortcut))
    ? offered
    : [...offered, dictation.shortcut];
  // Handy listening for the same key: both would answer it.
  const sharedWithHandy =
    status.handy_running && takenBy(dictation.shortcut) === "handy";

  const chosenTidy = status.tidy_choices.find(
    (choice) =>
      providerKey(choice.provider) === providerKey(dictation.tidy_with),
  );
  const anyFast = status.tidy_choices.some((choice) => choice.speed === "fast");
  const speedName = (speed: Speed) => t(`dictation.speed.${speed}`);

  return (
    <section id="dictation" aria-labelledby="dictation-title">
      <h2 id="dictation-title" className="set-h">
        {t("dictation.title")}
      </h2>
      <div className="srow">
        <div>
          <div className="l">{t("dictation.enabled")}</div>
          <div className="h">{t("dictation.lead")}</div>
        </div>
        <div className="v">
          <input
            type="checkbox"
            role="switch"
            className="tg"
            checked={dictation.enabled}
            aria-label={t("dictation.enabled")}
            onChange={(event) =>
              void save({ ...dictation, enabled: event.target.checked })
            }
          />
        </div>
      </div>

      {/* The rest only matters once dictation is on. */}
      {dictation.enabled ? (
        <>
          {!status.permitted ? (
            <div className="srow">
              <div>
                <div className="l">{t("dictation.permission")}</div>
                <div className="h rt">{t("dictation.permissionNeeded")}</div>
              </div>
              <div className="v">
                <button
                  type="button"
                  className="btn"
                  onClick={() => void commands.seogiOpenTypingPermission()}
                >
                  {t("dictation.permissionOpen")}
                </button>
              </div>
            </div>
          ) : null}

          <div className="srow">
            <div>
              <div className="l">{t("dictation.shortcut")}</div>
              <div className="h">
                {t("dictation.shortcutHint")}
                {!mac && keySet(dictation.shortcut) === keySet("alt+space") ? (
                  <span className="block">{t("dictation.altSpaceNote")}</span>
                ) : null}
                {sharedWithHandy ? (
                  <span className="block rt">
                    {t("dictation.handyRunning", {
                      keys: keyLabel(dictation.shortcut, mac),
                    })}
                  </span>
                ) : null}
                {status.shortcut_trouble ? (
                  <span className="block rt">
                    {t("dictation.shortcutTrouble", {
                      keys: keyLabel(dictation.shortcut, mac),
                    })}
                  </span>
                ) : null}
              </div>
            </div>
            <div className="v">
              <select
                className="sel"
                aria-label={t("dictation.shortcut")}
                value={
                  keys.find((k) => keySet(k) === keySet(dictation.shortcut)) ??
                  dictation.shortcut
                }
                onChange={(event) =>
                  void save({ ...dictation, shortcut: event.target.value })
                }
              >
                {keys.map((k, index) => {
                  const by = takenBy(k);
                  const mine = keySet(k) === keySet(dictation.shortcut);
                  // Seogi's own keys cannot be taken; Handy's can (Seogi
                  // replaces it), with a warning while Handy runs.
                  const blocked = !!by && by !== "handy" && !mine;
                  return (
                    <option key={k} value={k} disabled={blocked}>
                      {keyLabel(k, mac)}
                      {index === 0
                        ? ` (${t("dictation.shortcutDefault")})`
                        : ""}
                      {by && (by !== "handy" || status.handy_running)
                        ? ` — ${t(`dictation.shortcutTaken.${by}`)}`
                        : ""}
                    </option>
                  );
                })}
              </select>
            </div>
          </div>

          <div className="srow">
            <div>
              <div className="l">{t("dictation.press")}</div>
              <div className="h">
                {t(`dictation.pressHint.${dictation.press}`)}
              </div>
            </div>
            <div className="v">
              <div
                className="seg"
                role="radiogroup"
                aria-label={t("dictation.press")}
              >
                {PRESSES.map((press) => (
                  <button
                    key={press}
                    type="button"
                    role="radio"
                    aria-checked={dictation.press === press}
                    onClick={() => void save({ ...dictation, press })}
                  >
                    {t(`dictation.pressName.${press}`)}
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="srow">
            <div>
              <div className="l">{t("dictation.typing")}</div>
              <div className="h">
                {t(`dictation.typingHint.${dictation.typing}`)}
              </div>
            </div>
            <div className="v">
              <div
                className="seg"
                role="radiogroup"
                aria-label={t("dictation.typing")}
              >
                {TYPINGS.map((typing) => (
                  <button
                    key={typing}
                    type="button"
                    role="radio"
                    aria-checked={dictation.typing === typing}
                    onClick={() => void save({ ...dictation, typing })}
                  >
                    {t(`dictation.typingName.${typing}`)}
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="srow">
            <div>
              <div className="l">{t("dictation.model")}</div>
              <div className="h">{t("dictation.modelHint")}</div>
            </div>
            <div className="v">
              {downloads[dictation.model] !== undefined ? (
                <span className="t3">
                  {t("dictation.downloading", {
                    percent: Math.round(downloads[dictation.model] ?? 0),
                  })}
                </span>
              ) : null}
              <select
                className="sel max-w-[300px]"
                aria-label={t("dictation.model")}
                value={dictation.model}
                onChange={(event) =>
                  void save({ ...dictation, model: event.target.value })
                }
              >
                {status.models.map((model) => (
                  <option key={model.id} value={model.id}>
                    {[
                      model.label,
                      `· ${speedName(model.speed)}`,
                      model.accurate ? `· ${t("dictation.accurate")}` : "",
                      model.recommended
                        ? `· ${t("dictation.recommended")}`
                        : "",
                      model.downloaded
                        ? ""
                        : `· ${t("dictation.notDownloaded", {
                            size: `${(model.size_mb / 1000).toFixed(1)}GB`,
                          })}`,
                    ]
                      .filter(Boolean)
                      .join(" ")}
                  </option>
                ))}
              </select>
            </div>
          </div>

          <div className="srow">
            <div>
              <div className="l">{t("dictation.fillers")}</div>
              <div className="h">{t("dictation.fillersHint")}</div>
            </div>
            <div className="v">
              <input
                type="checkbox"
                role="switch"
                className="tg"
                checked={dictation.fillers}
                aria-label={t("dictation.fillers")}
                onChange={(event) =>
                  void save({ ...dictation, fillers: event.target.checked })
                }
              />
            </div>
          </div>

          <div className="srow">
            <div>
              <div className="l">{t("dictation.numbers")}</div>
              <div className="h">{t("dictation.numbersHint")}</div>
            </div>
            <div className="v">
              <input
                type="checkbox"
                role="switch"
                className="tg"
                checked={dictation.numbers}
                aria-label={t("dictation.numbers")}
                onChange={(event) =>
                  void save({ ...dictation, numbers: event.target.checked })
                }
              />
            </div>
          </div>

          <div className="srow">
            <div>
              <div className="l">{t("dictation.tidy")}</div>
              <div className="h">
                {t("dictation.tidyHint")}
                <span className="block">
                  {chosenTidy?.metered
                    ? t("dictation.tidyMetered")
                    : t("dictation.tidyFree")}
                </span>
                {!anyFast ? (
                  <span className="block">{t("dictation.tidyNoFast")}</span>
                ) : null}
              </div>
            </div>
            <div className="v">
              <input
                type="checkbox"
                role="switch"
                className="tg"
                checked={dictation.tidy}
                aria-label={t("dictation.tidy")}
                onChange={(event) =>
                  void save({ ...dictation, tidy: event.target.checked })
                }
              />
            </div>
          </div>
          {dictation.tidy ? (
            <div className="srow">
              <div>
                <div className="l">{t("dictation.tidyWith")}</div>
                <div
                  className={`h ${chosenTidy?.speed === "slow" ? "rt" : ""}`}
                >
                  {t("dictation.tidyWithHint")}
                </div>
              </div>
              <div className="v">
                {chosenTidy?.last_ms ? (
                  <span className="t3">
                    {t("dictation.tidyLast", {
                      s: (chosenTidy.last_ms / 1000).toFixed(1),
                    })}
                  </span>
                ) : null}
                <select
                  className="sel max-w-[300px]"
                  aria-label={t("dictation.tidyWith")}
                  value={providerKey(dictation.tidy_with)}
                  onChange={(event) => {
                    const chosen = status.tidy_choices.find(
                      (choice) =>
                        providerKey(choice.provider) === event.target.value,
                    );
                    if (chosen)
                      void save({ ...dictation, tidy_with: chosen.provider });
                  }}
                >
                  {status.tidy_choices.map((choice) => (
                    <option
                      key={providerKey(choice.provider)}
                      value={providerKey(choice.provider)}
                    >
                      {[
                        choice.label,
                        `· ${speedName(choice.speed)}`,
                        choice.speed === "fast"
                          ? `· ${t("dictation.recommended")}`
                          : choice.speed === "slow"
                            ? `(${t("dictation.tidySlowWarn")})`
                            : "",
                      ]
                        .filter(Boolean)
                        .join(" ")}
                    </option>
                  ))}
                </select>
              </div>
            </div>
          ) : null}

          <div className="srow">
            <div>
              <div className="l">{t("dictation.sounds")}</div>
              <div className="h">{t("dictation.soundsHint")}</div>
            </div>
            <div className="v">
              <input
                type="checkbox"
                role="switch"
                className="tg"
                checked={dictation.sounds}
                aria-label={t("dictation.sounds")}
                onChange={(event) =>
                  void save({ ...dictation, sounds: event.target.checked })
                }
              />
            </div>
          </div>

          <div className="srow">
            <div>
              <div className="l">{t("dictation.button")}</div>
              <div className="h">{t("dictation.buttonHint")}</div>
            </div>
            <div className="v">
              <input
                type="checkbox"
                role="switch"
                className="tg"
                checked={dictation.button}
                aria-label={t("dictation.button")}
                onChange={(event) =>
                  void save({ ...dictation, button: event.target.checked })
                }
              />
            </div>
          </div>

          <div className="srow">
            <div>
              <div className="l">{t("dictation.keep")}</div>
              <div className="h">
                {t("dictation.keepHint", { n: dictation.keep })}
              </div>
            </div>
            <div className="v">
              {status.recent.length > 0 && askingAll ? (
                <>
                  <span>{t("dictation.forgetAllAsk")}</span>
                  <button
                    type="button"
                    className="btn"
                    onClick={() => void forget(null)}
                  >
                    {t("dictation.forget")}
                  </button>
                  <button
                    type="button"
                    className="lnk"
                    onClick={() => setAskingAll(false)}
                  >
                    {t("dictation.forgetCancel")}
                  </button>
                </>
              ) : status.recent.length > 0 ? (
                <button
                  type="button"
                  className="lnk"
                  onClick={() => setAskingAll(true)}
                >
                  {t("dictation.forgetAll")}
                </button>
              ) : null}
            </div>
            <div className="full grid gap-1.5 text-[13.5px]">
              {status.recent.length === 0 ? (
                <p className="m-0 t3">{t("dictation.keepNone")}</p>
              ) : (
                status.recent.map((item) => (
                  <div key={item.id} className="flex items-start gap-3">
                    <span className="t3 shrink-0 tabular-nums">
                      {new Date(item.at_ms).toLocaleTimeString([], {
                        hour: "2-digit",
                        minute: "2-digit",
                      })}
                    </span>
                    <span className="flex-1 min-w-0 whitespace-pre-line">
                      {item.text}
                    </span>
                    <button
                      type="button"
                      className="lnk"
                      onClick={() =>
                        void navigator.clipboard
                          .writeText(item.text)
                          .then(() => onNote(t("dictation.copied")))
                      }
                    >
                      {t("dictation.copy")}
                    </button>
                    <button
                      type="button"
                      className="lnk"
                      onClick={() => void forget([item.id])}
                    >
                      {t("dictation.forget")}
                    </button>
                  </div>
                ))
              )}
            </div>
          </div>
        </>
      ) : null}
    </section>
  );
}
