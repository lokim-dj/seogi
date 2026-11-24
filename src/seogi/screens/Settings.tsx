// Settings: one column of rows, each a name, a line saying what goes in it,
// and its value. What is rarely changed waits under "더 보기". Each change
// is saved as it is made.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { platform } from "@tauri-apps/plugin-os";
import {
  commands,
  type CalendarCheck,
  type CalendarStatus,
  type Config,
  type MacCalendar,
  type Microphone,
  type SummaryTry,
} from "@/bindings";
import { BackIcon, RightIcon } from "../components/icons";
import DictationSettings from "./DictationSettings";
import { NS } from "../i18n";
import {
  friendlyPath,
  hm,
  keysLabel,
  longDay,
  shortModel,
} from "../lib/format";
import {
  KEY_OWNERS,
  keyOwnerOf,
  LOCAL,
  loginProvider,
  modelOptions,
  modeOf,
  selectedModel,
  type KeyOwner,
  type Mode,
} from "../lib/summary";
import { useSeogi } from "../store";

const MODES: Mode[] = ["local", "login", "key"];

export default function Settings() {
  const { t, i18n } = useTranslation(NS);
  const config = useSeogi((s) => s.config);
  const readiness = useSeogi((s) => s.readiness);
  const downloads = useSeogi((s) => s.downloads);
  const saveConfig = useSeogi((s) => s.saveConfig);
  const refreshSetup = useSeogi((s) => s.refreshSetup);
  const go = useSeogi((s) => s.go);
  const [note, setNote] = useState("");
  const [key, setKey] = useState("");
  const [hasKey, setHasKey] = useState(false);
  const [newUrl, setNewUrl] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [calendarTrouble, setCalendarTrouble] = useState("");
  const [calendars, setCalendars] = useState<CalendarStatus>({
    mac: null,
    addresses: [],
  });
  const [macAccess, setMacAccess] = useState("");
  const [macCalendars, setMacCalendars] = useState<MacCalendar[]>([]);
  const [asking, setAsking] = useState(false);
  const [microphones, setMicrophones] = useState<Microphone[]>([]);
  // What the last "시험해 보기" gave: null while not tried, "…" while asking.
  const [tried, setTried] = useState<SummaryTry | "asking" | null>(null);
  const mac = platform() === "macos";

  const owner: KeyOwner = config ? keyOwnerOf(config.summary) : "anthropic";
  const mode: Mode = config ? modeOf(config.summary) : "local";

  useEffect(() => {
    void commands.seogiMicrophones().then((result) => {
      if (result.status === "ok") setMicrophones(result.data);
    });
  }, []);

  // How each saved calendar answered. The calendars are read in the
  // background after a change, so this looks again for a little while.
  const calendarCount = config?.calendar_urls.length ?? 0;
  const macChosen = (config?.mac_calendars.length ?? 0) > 0;
  const macKey = config?.mac_calendars.join("|") ?? "";
  useEffect(() => {
    let stop = false;
    let tries = 0;
    const look = async () => {
      const status = await commands.seogiCalendarStatus();
      if (stop) return;
      setCalendars(status);
      tries += 1;
      const behind =
        status.addresses.length < calendarCount || (macChosen && !status.mac);
      if (behind && tries < 20) {
        window.setTimeout(() => void look(), 1500);
      }
    };
    void look();
    return () => {
      stop = true;
    };
  }, [calendarCount, macChosen, macKey]);

  // The Calendar app's calendars, once Seogi may read them.
  const listMac = async () => {
    const result = await commands.seogiMacCalendars();
    if (result.status === "ok") setMacCalendars(result.data);
  };
  useEffect(() => {
    if (!mac) return;
    void commands.seogiMacCalendarAccess().then((access) => {
      setMacAccess(access);
      if (access === "granted") void listMac();
    });
  }, [mac]);

  useEffect(() => {
    if (mode !== "key") return;
    void commands.seogiHasKey(owner).then((result) => {
      setHasKey(result.status === "ok" && result.data);
    });
  }, [mode, owner]);

  if (!config) return null;

  const save = async (next: Config) => {
    const error = await saveConfig(next);
    setNote(error ? t("failed", { reason: error }) : t("settings.saved"));
  };

  const chooseMode = (next: Mode) => {
    if (next === mode) return;
    if (next === "local") void save({ ...config, summary: LOCAL });
    if (next === "login") {
      const tool =
        readiness?.claude_cli === false && readiness?.codex_cli
          ? "codex"
          : "claude";
      void save({ ...config, summary: loginProvider(tool, readiness) });
    }
    if (next === "key") {
      const first = KEY_OWNERS[0];
      void save({ ...config, summary: first.provider(first.model) });
    }
  };

  const setModel = (model: string) => {
    const summary = config.summary;
    void save({ ...config, summary: { ...summary, model } });
  };

  const saveKey = async () => {
    const result = await commands.seogiSetKey(owner, key.trim());
    if (result.status === "error") {
      setNote(t("failed", { reason: result.error }));
      return;
    }
    setKey("");
    setHasKey(true);
    setNote(t("settings.saved"));
  };

  const pick = async (directory: boolean) => {
    const picked = await open({ directory, multiple: false });
    return typeof picked === "string" ? picked : null;
  };

  const addCalendar = async () => {
    const source = newUrl.trim();
    if (!source || connecting) return;
    setConnecting(true);
    setCalendarTrouble("");
    const result = await commands.seogiCheckCalendar(source);
    setConnecting(false);
    if (result.status !== "ok" || !result.data.ok) {
      const reason = result.status === "ok" ? result.data.reason : "";
      setCalendarTrouble(
        t("settings.calendarFailed", {
          why: t(`settings.calendarReason.${reason}`, {
            defaultValue: t("onboarding.calendarBadHint"),
          }),
        }),
      );
      return;
    }
    const address = result.data.address || source;
    setNewUrl("");
    if (config.calendar_urls.includes(address)) return;
    const error = await saveConfig({
      ...config,
      calendar_urls: [...config.calendar_urls, address],
    });
    if (error) {
      setNote(t("failed", { reason: error }));
      return;
    }
    // Read now, so the meetings can be named from it straight away.
    await commands.seogiRefreshCalendar();
    setCalendars(await commands.seogiCalendarStatus());
    setNote(t("settings.calendarAdded"));
  };

  const askMac = async () => {
    setAsking(true);
    const result = await commands.seogiMacCalendarRequest();
    setAsking(false);
    const access = result.status === "ok" ? result.data : "denied";
    setMacAccess(access);
    if (access === "granted") await listMac();
  };

  const chooseMac = async (id: string, on: boolean) => {
    const chosen = on
      ? [...config.mac_calendars.filter((other) => other !== id), id]
      : config.mac_calendars.filter((other) => other !== id);
    const error = await saveConfig({ ...config, mac_calendars: chosen });
    if (error) {
      setNote(t("failed", { reason: error }));
      return;
    }
    await commands.seogiRefreshCalendar();
    setCalendars(await commands.seogiCalendarStatus());
    setNote(t("settings.saved"));
  };

  /** "연결됨 · 오늘 일정 3개 · 다음 일정: …", or why it could not be read. */
  const describe = (status: CalendarCheck) =>
    status.ok
      ? [
          t("settings.calendarToday", { n: status.today }),
          status.next
            ? t("settings.calendarNext", {
                when: `${longDay(status.next.start_ms, i18n.language)} ${hm(status.next.start_ms)}`,
                title: status.next.title,
              })
            : t("settings.calendarNoNext"),
        ].join(" · ")
      : t(`settings.calendarReason.${status.reason}`, {
          defaultValue: t("onboarding.calendarBadHint"),
        });

  const calendarLine = (index: number) => {
    const status = calendars.addresses[index];
    if (!status)
      return {
        head: t("settings.calendarWaiting", { n: index + 1 }),
        body: "",
        ok: true,
      };
    return {
      head: t(status.ok ? "settings.calendarOk" : "settings.calendarBad", {
        n: index + 1,
      }),
      body: describe(status),
      ok: status.ok,
    };
  };

  const options = modelOptions(config.summary, readiness);

  const inUse = options.find(
    (option) => option.id === selectedModel(config.summary, options),
  );
  const tryIt = async () => {
    setTried("asking");
    const result = await commands.seogiTrySummary();
    setTried(result.status === "ok" ? result.data : null);
    if (result.status === "error")
      setNote(t("failed", { reason: result.error }));
  };

  const speechPercent = readiness
    ? Math.round(downloads[readiness.speech_model] ?? -1)
    : -1;
  const modelHint = readiness?.summary_model_refused
    ? t("settings.modelRefused", { model: config.summary.model })
    : options.find(
          (option) => option.id === selectedModel(config.summary, options),
        )?.aboutKey
      ? t(
          options.find(
            (option) => option.id === selectedModel(config.summary, options),
          )?.aboutKey ?? "",
        )
      : t("settings.modelHint");

  return (
    <div className="min-h-full pb-20">
      <header className="a-bar">
        <button
          type="button"
          className="ib !pl-0.5"
          onClick={() => go({ name: "home" })}
        >
          <BackIcon />
          {t("home.box")}
        </button>
        <span role="status" className="t3 text-[13px]">
          {note}
        </span>
      </header>

      <main className="a-col pt-3.5">
        <h1 className="a-h1">{t("settings.title")}</h1>

        <h2 className="set-h">{t("settings.summary")}</h2>
        <div className="srow">
          <div>
            <div className="l">{t("settings.who")}</div>
            <div className="h">
              {t(`onboarding.content.${mode}`)} {t(`onboarding.money.${mode}`)}
            </div>
          </div>
          <div className="v">
            <div
              className="seg"
              role="radiogroup"
              aria-label={t("settings.who")}
            >
              {MODES.map((name) => (
                <button
                  key={name}
                  type="button"
                  role="radio"
                  aria-checked={mode === name}
                  onClick={() => chooseMode(name)}
                >
                  {t(`onboarding.${name}`)}
                </button>
              ))}
            </div>
          </div>
        </div>
        {mode === "login" ? (
          <div className="srow">
            <div>
              <div className="l">{t("onboarding.loginWith")}</div>
              <div className="h">{t("settings.loginHint")}</div>
            </div>
            <div className="v">
              <select
                aria-label={t("onboarding.loginWith")}
                value={config.summary.kind === "codex_cli" ? "codex" : "claude"}
                onChange={(event) =>
                  void save({
                    ...config,
                    summary: loginProvider(
                      event.target.value === "codex" ? "codex" : "claude",
                      readiness,
                    ),
                  })
                }
                className="sel"
              >
                <option value="claude">{"Claude"}</option>
                <option value="codex">{"ChatGPT (Codex)"}</option>
              </select>
            </div>
          </div>
        ) : null}
        {mode === "key" ? (
          <div className="srow">
            <div>
              <div className="l">{t("onboarding.keyFor")}</div>
              <div className="h">
                {hasKey ? t("onboarding.keySaved") : t("settings.keyHint")}
                {owner === "gemini" ? (
                  <span className="block rt">{t("onboarding.geminiWarn")}</span>
                ) : null}
              </div>
            </div>
            <div className="v">
              <select
                aria-label={t("onboarding.keyFor")}
                value={owner}
                onChange={(event) => {
                  const chosen = KEY_OWNERS.find(
                    (item) => item.id === event.target.value,
                  );
                  if (chosen) {
                    void save({
                      ...config,
                      summary: chosen.provider(chosen.model),
                    });
                  }
                }}
                className="sel"
              >
                {KEY_OWNERS.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.label}
                  </option>
                ))}
              </select>
            </div>
            <div className="full flex flex-wrap items-center gap-2.5">
              <input
                type="password"
                autoComplete="off"
                value={key}
                onChange={(event) => setKey(event.target.value)}
                placeholder={t("onboarding.keyField")}
                aria-label={t("onboarding.keyField")}
                className="ul flex-[1_1_220px] w-auto"
              />
              <button
                type="button"
                className="btn"
                disabled={!key.trim()}
                onClick={() => void saveKey()}
              >
                {t("settings.saveKey")}
              </button>
              {hasKey ? (
                <button
                  type="button"
                  className="lnk"
                  onClick={() =>
                    void commands
                      .seogiRemoveKey(owner)
                      .then(() => setHasKey(false))
                  }
                >
                  {t("settings.removeKey")}
                </button>
              ) : null}
            </div>
          </div>
        ) : null}
        <div className="srow">
          <div>
            <div className="l">{t("settings.model")}</div>
            <div
              className={`h ${readiness?.summary_model_refused ? "!text-rt" : ""}`}
            >
              {modelHint}
            </div>
          </div>
          <div className="v">
            {options.length > 0 ? (
              <select
                value={selectedModel(config.summary, options)}
                onChange={(event) => setModel(event.target.value)}
                aria-label={t("settings.model")}
                className="sel"
              >
                {options.map((option) => (
                  <option key={option.id} value={option.id}>
                    {option.recommended
                      ? `${option.label} ${t("settings.models.recommended")}`
                      : option.label}
                  </option>
                ))}
              </select>
            ) : (
              <span className="t3">{t("settings.modelNoOllama")}</span>
            )}
          </div>
        </div>
        <div className="srow">
          <div>
            <div className="l">{t("settings.state")}</div>
            <div className="h">
              {mode === "local" && readiness && !readiness.ollama_running
                ? t("onboarding.need.localNoOllama")
                : t("settings.stateHint")}
              <span className="block t2">
                {t("settings.inUse", {
                  who:
                    config.summary.kind === "claude_cli"
                      ? "Claude"
                      : config.summary.kind === "codex_cli"
                        ? "ChatGPT"
                        : mode === "key"
                          ? (KEY_OWNERS.find((item) => item.id === owner)
                              ?.label ?? t("onboarding.key"))
                          : t("onboarding.local"),
                  model: inUse?.label ?? config.summary.model,
                })}
              </span>
              {tried && tried !== "asking" ? (
                <span className={`block ${tried.ok ? "t2" : "rt"}`}>
                  {tried.ok
                    ? t("settings.triedOk", {
                        s: (tried.ms / 1000).toFixed(1),
                      })
                    : t(`trouble.${tried.trouble}`, {
                        defaultValue: t("trouble.summary_model"),
                      })}
                </span>
              ) : null}
            </div>
          </div>
          <div className="v">
            <span className={readiness?.summary_ready ? "t2" : "rt"}>
              {readiness?.summary_ready
                ? t("settings.ready")
                : t("settings.notReady")}
            </span>
            <button
              type="button"
              className="btn"
              disabled={tried === "asking"}
              onClick={() => void tryIt()}
            >
              {tried === "asking" ? t("settings.trying") : t("settings.tryIt")}
            </button>
            {mode === "local" &&
            readiness?.ollama_running &&
            !readiness.summary_ready ? (
              <button
                type="button"
                className="btn"
                onClick={() =>
                  void commands
                    .seogiPullSummaryModel()
                    .then(() => refreshSetup())
                }
              >
                {t("onboarding.need.pull")}
              </button>
            ) : null}
          </div>
        </div>

        <h2 className="set-h">{t("settings.speech")}</h2>
        <div className="srow">
          <div>
            <div className="l">{t("settings.diarize")}</div>
            <div className="h">
              {t("settings.diarizeHint")}
              {readiness && !readiness.diarizer_ready ? (
                <span className="block rt">
                  {t("settings.diarizerMissing")}
                </span>
              ) : null}
            </div>
          </div>
          <div className="v">
            {readiness && !readiness.diarizer_ready ? (
              <button
                type="button"
                className="btn"
                onClick={() =>
                  void commands
                    .seogiDownloadDiarizer()
                    .then(() => refreshSetup())
                }
              >
                {t("settings.download")}
              </button>
            ) : null}
            <input
              type="checkbox"
              role="switch"
              className="tg"
              checked={config.diarize}
              aria-label={t("settings.diarize")}
              onChange={(event) =>
                void save({ ...config, diarize: event.target.checked })
              }
            />
          </div>
        </div>
        <div className="srow">
          <div>
            <div className="l">{t("settings.vocabulary")}</div>
            <div className="h">
              {t("settings.vocabularyHint", {
                n: (readiness?.vocabulary_terms ?? 0).toLocaleString(),
              })}
            </div>
          </div>
          <div className="v">
            <span className="t2" title={readiness?.vocabulary_path}>
              {friendlyPath(readiness?.vocabulary_path ?? "")
                .split(" › ")
                .pop()}
            </span>
            <button
              type="button"
              className="lnk"
              onClick={() =>
                void pick(true).then((path) => {
                  if (path) void save({ ...config, vocabulary_path: path });
                })
              }
            >
              {t("settings.change")}
            </button>
          </div>
        </div>
        <div className="srow">
          <div>
            <div className="l">{t("settings.microphone")}</div>
            <div className="h">{t("settings.micHint")}</div>
          </div>
          <div className="v">
            <select
              value={config.input_device}
              onChange={(event) =>
                void save({ ...config, input_device: event.target.value })
              }
              aria-label={t("settings.microphone")}
              className="sel max-w-[220px]"
            >
              <option value="">{t("settings.micDefault")}</option>
              {microphones.map((microphone) => (
                <option key={microphone.id} value={microphone.id}>
                  {microphone.name}
                </option>
              ))}
            </select>
          </div>
        </div>

        <h2 className="set-h">{t("settings.keepAndCalendar")}</h2>
        <div className="srow">
          <div>
            <div className="l">{t("settings.library")}</div>
            <div className="h">{t("settings.libraryHint")}</div>
          </div>
          <div className="v">
            <span className="t2" title={readiness?.library}>
              {friendlyPath(readiness?.library ?? "")}
            </span>
            <button
              type="button"
              className="lnk"
              onClick={() =>
                void pick(true).then((path) => {
                  if (path) void save({ ...config, library_dir: path });
                })
              }
            >
              {t("settings.change")}
            </button>
          </div>
        </div>
        <div className="srow">
          <div>
            <div className="l">{t("settings.inbox")}</div>
            <div className="h">{t("settings.inboxHint")}</div>
          </div>
          <div className="v">
            <button
              type="button"
              className="lnk"
              onClick={() =>
                void pick(true).then(async (path) => {
                  if (!path) return;
                  const result = await commands.seogiWatchFolder(path);
                  setNote(
                    result.status === "error"
                      ? t("failed", { reason: result.error })
                      : t("settings.saved"),
                  );
                  await refreshSetup();
                })
              }
            >
              {t("settings.add")}
            </button>
          </div>
          {config.watched.length > 0 ||
          (readiness?.drives ?? []).some(
            (account) =>
              !config.watched.some((folder) => folder.path === account.inbox),
          ) ? (
            <div className="full grid gap-1.5 text-[13.5px]">
              {config.watched.map((folder) => (
                <div key={folder.path} className="flex items-center gap-3">
                  <span
                    className="flex-1 min-w-0 truncate t2"
                    title={folder.path}
                  >
                    {friendlyPath(folder.path)}
                  </span>
                  <button
                    type="button"
                    className="lnk"
                    onClick={() =>
                      void save({
                        ...config,
                        watched: config.watched.filter(
                          (other) => other.path !== folder.path,
                        ),
                      })
                    }
                  >
                    {t("settings.remove")}
                  </button>
                </div>
              ))}
              {(readiness?.drives ?? [])
                .filter(
                  (account) =>
                    !config.watched.some(
                      (folder) => folder.path === account.inbox,
                    ),
                )
                .map((account) => (
                  <button
                    key={account.inbox}
                    type="button"
                    className="lnk justify-self-start"
                    onClick={() =>
                      void commands
                        .seogiWatchFolder(account.inbox)
                        .then(async (result) => {
                          setNote(
                            result.status === "error"
                              ? t("failed", { reason: result.error })
                              : t("settings.saved"),
                          );
                          await refreshSetup();
                        })
                    }
                  >
                    {t("settings.inboxMakeIn", { account: account.account })}
                  </button>
                ))}
            </div>
          ) : null}
        </div>
        <div className="srow">
          <div>
            <div className="l">{t("settings.calendar")}</div>
            <div className="h">
              {mac ? t("settings.calendarHintMac") : t("settings.calendarHint")}
            </div>
          </div>
          <div className="v">
            {mac && macAccess === "granted" && calendars.mac && macChosen ? (
              <span className={calendars.mac.ok ? "t2" : "rt"}>
                {calendars.mac.ok
                  ? t("settings.macOkShort")
                  : t("settings.macBad")}
              </span>
            ) : mac &&
              macAccess !== "granted" &&
              macAccess !== "denied" &&
              macAccess !== "restricted" ? (
              <button
                type="button"
                className="btn"
                disabled={asking}
                onClick={() => void askMac()}
              >
                {t("settings.macConnect")}
              </button>
            ) : !mac && config.calendar_urls.length > 0 ? (
              <span className="t2">
                {t("settings.calendarCount", {
                  n: config.calendar_urls.length,
                })}
              </span>
            ) : null}
          </div>
          {mac ? (
            <div className="full text-[13.5px]">
              {asking ? (
                <p className="m-0 t2">{t("settings.macAsking")}</p>
              ) : null}
              {macAccess === "denied" || macAccess === "restricted" ? (
                <p className="m-0 rt">
                  {t(
                    macAccess === "denied"
                      ? "settings.macDenied"
                      : "settings.macRestricted",
                  )}
                </p>
              ) : null}
              {macAccess === "granted" ? (
                <>
                  {calendars.mac && macChosen ? (
                    <p className="m-0 mb-1.5 t3">{describe(calendars.mac)}</p>
                  ) : null}
                  {macCalendars.length === 0 ? (
                    <p className="m-0 t3">{t("settings.macNone")}</p>
                  ) : (
                    <div className="grid gap-1">
                      <span className="t3">{t("settings.macPick")}</span>
                      {macCalendars.map((calendar) => (
                        <label
                          key={calendar.id}
                          className="flex items-center gap-2.5 cursor-pointer"
                        >
                          <input
                            type="checkbox"
                            className="chk"
                            checked={config.mac_calendars.includes(calendar.id)}
                            onChange={(event) =>
                              void chooseMac(calendar.id, event.target.checked)
                            }
                          />
                          <span>{calendar.title}</span>
                          <span className="t3">{calendar.account}</span>
                        </label>
                      ))}
                    </div>
                  )}
                </>
              ) : null}
            </div>
          ) : null}
        </div>
        <div className="srow">
          <div>
            <div className="l">{t("settings.remind")}</div>
            <div className="h">{t("settings.remindHint")}</div>
          </div>
          <div className="v">
            {config.remind ? (
              <select
                value={config.remind_minutes_before}
                onChange={(event) =>
                  void save({
                    ...config,
                    remind_minutes_before: Number(event.target.value),
                  })
                }
                aria-label={t("settings.remind")}
                className="sel"
              >
                {[0, 1, 2, 5, 10].map((minutes) => (
                  <option key={minutes} value={minutes}>
                    {t("settings.remindBefore", { n: minutes })}
                  </option>
                ))}
              </select>
            ) : null}
            <input
              type="checkbox"
              role="switch"
              className="tg"
              checked={config.remind}
              aria-label={t("settings.remind")}
              onChange={(event) =>
                void save({ ...config, remind: event.target.checked })
              }
            />
          </div>
        </div>

        <DictationSettings onNote={setNote} />

        <details className="mt-6">
          <summary className="fold">
            <span>{t("settings.moreSettings")}</span>
            <span className="r">
              <RightIcon />
            </span>
          </summary>
          <div className="fold-body !pt-0">
            <div className="srow !border-t-0">
              <div>
                <div className="l">{t("settings.speechModel")}</div>
                <div className="h">{t("settings.speechModelHint")}</div>
              </div>
              <div className="v">
                <span className="t2">
                  {shortModel(readiness?.speech_model ?? "")}
                </span>
                {readiness?.speech_model_ready ? null : speechPercent >= 0 ? (
                  <span>
                    {t("settings.downloading", { percent: speechPercent })}
                  </span>
                ) : (
                  <button
                    type="button"
                    className="btn"
                    onClick={() =>
                      readiness &&
                      void commands.downloadModel(readiness.speech_model)
                    }
                  >
                    {t("settings.download")}
                  </button>
                )}
              </div>
            </div>
            <div className="srow">
              <div>
                <div className="l">{t("settings.vocabularyFile")}</div>
                <div className="h">{t("settings.vocabularyFileHint")}</div>
              </div>
              <div className="v">
                <button
                  type="button"
                  className="lnk"
                  onClick={() =>
                    void pick(false).then((path) => {
                      if (path) void save({ ...config, vocabulary_path: path });
                    })
                  }
                >
                  {t("settings.pickFile")}
                </button>
              </div>
            </div>
            <div className="srow">
              <div>
                <div className="l">{t("settings.calendarAddress")}</div>
                <div className="h">{t("onboarding.calendarHowText")}</div>
              </div>
              <div className="v" />
              <div className="full grid gap-1.5 text-[13.5px]">
                {config.calendar_urls.map((address, index) => {
                  const line = calendarLine(index);
                  return (
                    <div key={address} className="flex items-start gap-3">
                      <span className="flex-1 min-w-0">
                        <span className={line.ok ? "" : "rt"}>{line.head}</span>
                        {line.body ? (
                          <span className="block t3">{line.body}</span>
                        ) : null}
                      </span>
                      <button
                        type="button"
                        className="lnk"
                        onClick={() =>
                          void save({
                            ...config,
                            calendar_urls: config.calendar_urls.filter(
                              (other) => other !== address,
                            ),
                          })
                        }
                      >
                        {t("settings.remove")}
                      </button>
                    </div>
                  );
                })}
                <div className="flex items-center gap-2.5">
                  <input
                    type="text"
                    inputMode="url"
                    autoComplete="off"
                    spellCheck={false}
                    value={newUrl}
                    onChange={(event) => {
                      setNewUrl(event.target.value);
                      setCalendarTrouble("");
                    }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") void addCalendar();
                    }}
                    aria-label={t("onboarding.calendarField")}
                    placeholder="https://calendar.google.com/calendar/ical/…/basic.ics"
                    className="ul flex-1 w-auto"
                  />
                  <button
                    type="button"
                    className="btn"
                    disabled={!newUrl.trim() || connecting}
                    onClick={() => void addCalendar()}
                  >
                    {connecting
                      ? t("settings.calendarChecking")
                      : t("settings.calendarConnect")}
                  </button>
                </div>
                {calendarTrouble ? (
                  <p role="alert" className="m-0 rt">
                    {calendarTrouble}
                  </p>
                ) : null}
              </div>
            </div>
            <div className="srow">
              <div>
                <div className="l">{t("settings.rules")}</div>
                <div className="h">{t("settings.rulesHint")}</div>
              </div>
              <div className="v">
                <button
                  type="button"
                  className="lnk"
                  onClick={() => void commands.seogiOpenCalendarRules()}
                >
                  {t("settings.rulesOpen")}
                </button>
              </div>
            </div>
            {(
              [
                ["record_shortcut", "settings.recordKey"],
                ["mark_shortcut", "settings.markKey"],
              ] as const
            ).map(([field, label]) => (
              <div key={field} className="srow">
                <div>
                  <div className="l">{t(label)}</div>
                  <div className="h">
                    {keysLabel(config[field], mac)} ·{" "}
                    {t("settings.shortcutHint")}
                  </div>
                </div>
                <div className="v">
                  <input
                    type="text"
                    key={config[field]}
                    defaultValue={config[field]}
                    onBlur={(event) => {
                      const value = event.target.value.trim();
                      if (value !== config[field]) {
                        void save({ ...config, [field]: value });
                      }
                    }}
                    aria-label={t(label)}
                    className="ul !w-[140px] text-right"
                  />
                </div>
              </div>
            ))}
          </div>
        </details>
      </main>
    </div>
  );
}
