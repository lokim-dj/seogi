// Settings: the groups listed down the left (along the top in a narrow
// window), and the rows of the chosen group beside them, each a name, a
// line saying what goes in it, and its value. One group at a time keeps the
// page short; the group looked at last opens again. What is rarely changed
// waits under "더 보기". Each change is saved as it is made.

import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { platform } from "@tauri-apps/plugin-os";
import {
  commands,
  type CalendarCheck,
  type CalendarStatus,
  type Config,
  type LoginItem,
  type MacCalendar,
  type Microphone,
  type SignIns,
  type SummaryTry,
} from "@/bindings";
import { BackIcon, RightIcon } from "../components/icons";
import DictationSettings from "./DictationSettings";
import Permissions from "../components/Permissions";
import PhoneRow from "../components/PhoneRow";
import SignInRows from "../components/SignInRows";
import { NS } from "../i18n";
import {
  friendlyPath,
  hm,
  keysLabel,
  longDay,
  shortModel,
} from "../lib/format";
import { gigabytes, minutesToDownload, speechAbout } from "../lib/speech";
import SpeechDownload from "../components/SpeechDownload";
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
import { SETTINGS_PARTS, useSeogi, type SettingsPart } from "../store";

const MODES: Mode[] = ["local", "login", "key"];

/** Each group's name in the side list. */
const PART_NAMES: Record<SettingsPart, string> = {
  summary: "settings.summary",
  recording: "settings.speech",
  dictation: "dictation.title",
  keep: "settings.keepAndCalendar",
  start: "settings.startAndPermissions",
};

/** Where an arrow key in the side list moves to, or -1 for a key it does
 *  not answer. Alt+← stays the back key. */
function partMove(event: KeyboardEvent, at: number): number {
  if (event.altKey || event.metaKey || event.ctrlKey) return -1;
  const last = SETTINGS_PARTS.length - 1;
  if (event.key === "ArrowDown" || event.key === "ArrowRight")
    return Math.min(at + 1, last);
  if (event.key === "ArrowUp" || event.key === "ArrowLeft")
    return Math.max(at - 1, 0);
  if (event.key === "Home") return 0;
  if (event.key === "End") return last;
  return -1;
}

export default function Settings() {
  const { t, i18n } = useTranslation(NS);
  const config = useSeogi((s) => s.config);
  const readiness = useSeogi((s) => s.readiness);
  const downloads = useSeogi((s) => s.downloads);
  const saveConfig = useSeogi((s) => s.saveConfig);
  const refreshSetup = useSeogi((s) => s.refreshSetup);
  const go = useSeogi((s) => s.go);
  const part = useSeogi((s) => s.settingsPart);
  const setPart = useSeogi((s) => s.setSettingsPart);
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
  // Whether Seogi opens when the user logs in: what the setting wants, and
  // what the system has (it may be switched off in System Settings). Null
  // until read.
  const [atLogin, setAtLogin] = useState<boolean | null>(null);
  const [loginItem, setLoginItem] = useState<LoginItem | null>(null);
  // What the last "시험해 보기" gave: null while not tried, "…" while asking.
  const [tried, setTried] = useState<SummaryTry | "asking" | null>(null);
  const [keyModels, setKeyModels] = useState<string[]>([]);
  // Which "시험해 보기" is current, and how many keys were saved here: a
  // new key is read again even when one was saved before.
  const tryId = useRef(0);
  const [keySaves, setKeySaves] = useState(0);
  const mac = platform() === "macos";
  const signInsSeen = useRef("");

  const owner: KeyOwner = config ? keyOwnerOf(config.summary) : "anthropic";
  const mode: Mode = config ? modeOf(config.summary) : "local";

  useEffect(() => {
    void commands.seogiMicrophones().then((result) => {
      if (result.status === "ok") setMicrophones(result.data);
    });
  }, []);

  useEffect(() => {
    void commands.getAppSettings().then((result) => {
      if (result.status === "ok")
        setAtLogin(result.data.autostart_enabled ?? false);
    });
    void commands.seogiLoginItem().then(setLoginItem);
  }, []);

  // Switched off in System Settings: look again while the user is there.
  useEffect(() => {
    if (loginItem !== "needs_approval") return;
    const timer = window.setInterval(
      () => void commands.seogiLoginItem().then(setLoginItem),
      2000,
    );
    return () => window.clearInterval(timer);
  }, [loginItem]);

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

  // With a key saved, the company's own list of models: what it offers
  // now, besides the few named here.
  const summaryKey = config ? JSON.stringify(config.summary) : "";
  const keyProvider = mode === "key" && config ? config.summary : null;
  const keyProviderKind = keyProvider ? owner : "";
  useEffect(() => {
    setKeyModels([]);
    if (!keyProvider || !hasKey) return;
    let stop = false;
    void commands.seogiKeyModels(keyProvider).then((result) => {
      if (!stop && result.status === "ok") setKeyModels(result.data);
    });
    return () => {
      stop = true;
    };
    // Read again for another company or key, not for another model.
  }, [keyProviderKind, hasKey, keySaves]);

  // A try belongs to the way, company, model and key it was made with.
  useEffect(() => {
    tryId.current += 1;
    setTried(null);
  }, [summaryKey, hasKey, keySaves]);

  if (!config) return null;

  const onPartKey = (event: KeyboardEvent) => {
    const to = partMove(event, SETTINGS_PARTS.indexOf(part));
    if (to < 0) return;
    event.preventDefault();
    setPart(SETTINGS_PARTS[to]);
    document.getElementById(`set-tab-${SETTINGS_PARTS[to]}`)?.focus();
  };

  // A sign-in that changed changes whether minutes can be written: the
  // state row reads it again.
  const followSignIns = (now: SignIns) => {
    const key = `${now.claude}|${now.codex}`;
    if (signInsSeen.current && signInsSeen.current !== key) void refreshSetup();
    signInsSeen.current = key;
  };

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
    setKeySaves((n) => n + 1);
    void refreshSetup();
    setNote(t("settings.saved"));
  };

  // Saved and applied to the system's login items at once, so a login item
  // added by hand is not taken away again at the next start.
  const chooseAtLogin = async (enabled: boolean) => {
    setAtLogin(enabled);
    const result = await commands.changeAutostartSetting(enabled);
    // The switch shows what the system has now, whichever way it went.
    setLoginItem(await commands.seogiLoginItem());
    setNote(
      result.status === "error"
        ? t("settings.atLoginFailed")
        : t("settings.saved"),
    );
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

  const options = modelOptions(config.summary, readiness, keyModels);

  const inUse = options.find(
    (option) => option.id === selectedModel(config.summary, options),
  );
  const tryIt = async () => {
    const id = tryId.current;
    setTried("asking");
    const result = await commands.seogiTrySummary();
    // Something changed while it was asking: the answer is not for this.
    if (id !== tryId.current) return;
    setTried(result.status === "ok" ? result.data : null);
    if (result.status === "error")
      setNote(t("failed", { reason: result.error }));
  };

  // Read for the way chosen now: a key that is not saved yet is not ready,
  // whatever the last reading said.
  const needsKey = mode === "key" && !hasKey;
  const summaryReady = !needsKey && !!readiness?.summary_ready;
  // When the chosen way cannot write, the one that will instead.
  const fallback = readiness?.summary_fallback ?? null;
  const fallbackWay = !fallback
    ? ""
    : fallback.kind === "claude_cli"
      ? t("settings.way.claude")
      : fallback.kind === "codex_cli"
        ? t("settings.way.codex")
        : fallback.kind === "ollama"
          ? t("settings.way.local")
          : t("settings.way.key", {
              owner:
                KEY_OWNERS.find((item) => item.id === keyOwnerOf(fallback))
                  ?.label ?? "",
            });
  const fallbackNote = summaryReady
    ? null
    : fallback
      ? t(needsKey ? "settings.fallbackNoKey" : "settings.fallbackNotReady", {
          way: fallbackWay,
        })
      : mode !== "local" && readiness
        ? t("settings.noWay")
        : null;

  const speechPercent = readiness
    ? Math.round(downloads[readiness.speech_model] ?? -1)
    : -1;
  // The meetings' models to choose from: the default (first), whether or
  // not it is here, and the others that are downloaded.
  const speechChoices = (readiness?.speech_models ?? []).filter(
    (option, index) => index === 0 || option.downloaded,
  );
  const chosenSpeech = config.speech_model.trim() || speechChoices[0]?.id || "";
  const about = speechAbout(readiness?.speech_model ?? "");
  // What the row says under the model while it is not here: its size and
  // time, and what transcribes meanwhile.
  const speechWaitNote =
    !readiness || readiness.speech_model_ready
      ? null
      : [
          readiness.speech_model_bytes > 0 && speechPercent < 0
            ? t("settings.speechSize", {
                gb: gigabytes(readiness.speech_model_bytes),
                minutes: minutesToDownload(readiness.speech_model_bytes),
              })
            : null,
          readiness.speech_model_stand_in
            ? t("settings.speechStandIn", {
                model: shortModel(readiness.speech_model_stand_in),
              })
            : t("settings.speechNoStandIn"),
        ]
          .filter(Boolean)
          .join(" · ");
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

      <main className="set-wrap pt-3.5">
        <h1 className="a-h1">{t("settings.title")}</h1>
        <div className="set-grid">
          <div
            role="tablist"
            aria-label={t("settings.parts")}
            aria-orientation="vertical"
            className="set-nav"
            onKeyDown={onPartKey}
          >
            {SETTINGS_PARTS.map((name) => (
              <button
                key={name}
                type="button"
                role="tab"
                id={`set-tab-${name}`}
                aria-selected={part === name}
                aria-controls="set-pane"
                tabIndex={part === name ? 0 : -1}
                onClick={() => setPart(name)}
              >
                {t(PART_NAMES[name])}
              </button>
            ))}
          </div>
          <div
            role="tabpanel"
            id="set-pane"
            aria-labelledby={`set-tab-${part}`}
            className="set-pane"
          >
            {part === "summary" ? (
              <>
                <h2 className="set-h">{t("settings.summary")}</h2>
                <div className="srow">
                  <div>
                    <div className="l">{t("settings.who")}</div>
                    <div className="h">
                      {t(`onboarding.content.${mode}`)}{" "}
                      {t(`onboarding.money.${mode}`)}
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
                      <div className="h">
                        {t("settings.loginHint")}
                        <span className="block">{t("signIn.noPlan")}</span>
                      </div>
                    </div>
                    <div className="v">
                      <select
                        aria-label={t("onboarding.loginWith")}
                        value={
                          config.summary.kind === "codex_cli"
                            ? "codex"
                            : "claude"
                        }
                        onChange={(event) =>
                          void save({
                            ...config,
                            summary: loginProvider(
                              event.target.value === "codex"
                                ? "codex"
                                : "claude",
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
                {mode === "login" ? (
                  <SignInRows onChange={followSignIns} />
                ) : null}
                {mode === "key" ? (
                  <div className="srow">
                    <div>
                      <div className="l">{t("onboarding.keyFor")}</div>
                      <div className="h">
                        {hasKey
                          ? t("onboarding.keySaved")
                          : t("settings.keyHint")}
                        {owner === "gemini" ? (
                          <span className="block rt">
                            {t("onboarding.geminiWarn")}
                          </span>
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
                            void commands.seogiRemoveKey(owner).then(() => {
                              setHasKey(false);
                              void refreshSetup();
                            })
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
                      {mode === "local" &&
                      readiness &&
                      !readiness.ollama_running
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
                                  ? (KEY_OWNERS.find(
                                      (item) => item.id === owner,
                                    )?.label ?? t("onboarding.key"))
                                  : t("onboarding.local"),
                          model: inUse?.label ?? config.summary.model,
                        })}
                      </span>
                      {fallbackNote ? (
                        <span className={`block ${fallback ? "t2" : "rt"}`}>
                          {fallbackNote}
                        </span>
                      ) : null}
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
                    <span className={summaryReady ? "t2" : "rt"}>
                      {summaryReady
                        ? t("settings.ready")
                        : needsKey
                          ? t("settings.needsKey")
                          : t("settings.notReady")}
                    </span>
                    <button
                      type="button"
                      className="btn"
                      disabled={tried === "asking"}
                      onClick={() => void tryIt()}
                    >
                      {tried === "asking"
                        ? t("settings.trying")
                        : t("settings.tryIt")}
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
              </>
            ) : null}
            {part === "recording" ? (
              <>
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
                          if (path)
                            void save({ ...config, vocabulary_path: path });
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
                        void save({
                          ...config,
                          input_device: event.target.value,
                        })
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
                <More>
                  <div className="srow !border-t-0">
                    <div>
                      <div className="l">{t("settings.speechModel")}</div>
                      <div className="h">
                        {/* With the choices below, each says its own. */}
                        {about && speechChoices.length <= 1
                          ? t(`settings.speechAbout.${about}`)
                          : t("settings.speechModelHint")}
                      </div>
                      {speechWaitNote ? (
                        <div className="h">{speechWaitNote}</div>
                      ) : null}
                    </div>
                    <div className="v">
                      <span className="t2">
                        {shortModel(readiness?.speech_model ?? "")}
                      </span>
                      {readiness && !readiness.speech_model_ready ? (
                        <SpeechDownload model={readiness.speech_model} />
                      ) : null}
                    </div>
                  </div>
                  {speechChoices.length > 1 ? (
                    <div className="srow !border-t-0 !pt-0">
                      <div
                        role="radiogroup"
                        aria-label={t("settings.speechModel")}
                        className="flex flex-col gap-2 w-full"
                      >
                        {speechChoices.map((option, index) => {
                          const aboutKey = speechAbout(option.id);
                          return (
                            <label
                              key={option.id}
                              className="flex items-start gap-2 cursor-pointer"
                            >
                              <input
                                type="radio"
                                name="speech-model"
                                checked={chosenSpeech === option.id}
                                onChange={() =>
                                  void save({
                                    ...config,
                                    // The default is followed, not pinned.
                                    speech_model: index === 0 ? "" : option.id,
                                  })
                                }
                                className="w-3.5 h-3.5 mt-[3px] m-0 accent-[var(--red)]"
                              />
                              <span>
                                <span className="text-[14px] font-medium">
                                  {shortModel(option.id)}
                                  {index === 0
                                    ? ` · ${t("settings.speechDefault")}`
                                    : ""}
                                </span>
                                <span className="h block">
                                  {aboutKey
                                    ? t(`settings.speechAbout.${aboutKey}`)
                                    : null}
                                  {option.downloaded
                                    ? null
                                    : ` · ${t("settings.speechNotHere", {
                                        gb: gigabytes(option.bytes),
                                      })}`}
                                </span>
                              </span>
                            </label>
                          );
                        })}
                      </div>
                    </div>
                  ) : null}
                  <div className="srow">
                    <div>
                      <div className="l">{t("settings.vocabularyFile")}</div>
                      <div className="h">
                        {t("settings.vocabularyFileHint")}
                      </div>
                    </div>
                    <div className="v">
                      <button
                        type="button"
                        className="lnk"
                        onClick={() =>
                          void pick(false).then((path) => {
                            if (path)
                              void save({ ...config, vocabulary_path: path });
                          })
                        }
                      >
                        {t("settings.pickFile")}
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
                </More>
              </>
            ) : null}
            {part === "dictation" ? (
              <DictationSettings onNote={setNote} />
            ) : null}
            {part === "keep" ? (
              <>
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
                      !config.watched.some(
                        (folder) => folder.path === account.inbox,
                      ),
                  ) ? (
                    <div className="full grid gap-1.5 text-[13.5px]">
                      {config.watched.map((folder) => (
                        <div
                          key={folder.path}
                          className="flex items-center gap-3"
                        >
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
                            {t("settings.inboxMakeIn", {
                              account: account.account,
                            })}
                          </button>
                        ))}
                    </div>
                  ) : null}
                </div>
                <PhoneRow onNote={setNote} />
                <div className="srow">
                  <div>
                    <div className="l">{t("settings.calendar")}</div>
                    <div className="h">
                      {mac
                        ? t("settings.calendarHintMac")
                        : t("settings.calendarHint")}
                    </div>
                  </div>
                  <div className="v">
                    {mac &&
                    macAccess === "granted" &&
                    calendars.mac &&
                    macChosen ? (
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
                            <p className="m-0 mb-1.5 t3">
                              {describe(calendars.mac)}
                            </p>
                          ) : null}
                          {macCalendars.length === 0 ? (
                            <p className="m-0 t3">{t("settings.macNone")}</p>
                          ) : (
                            <div className="grid gap-1">
                              <span className="t3">
                                {t("settings.macPick")}
                              </span>
                              {macCalendars.map((calendar) => (
                                <label
                                  key={calendar.id}
                                  className="flex items-center gap-2.5 cursor-pointer"
                                >
                                  <input
                                    type="checkbox"
                                    className="chk"
                                    checked={config.mac_calendars.includes(
                                      calendar.id,
                                    )}
                                    onChange={(event) =>
                                      void chooseMac(
                                        calendar.id,
                                        event.target.checked,
                                      )
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
                {config.muted_events.length > 0 ? (
                  <MutedEvents uids={config.muted_events} />
                ) : null}
                <More>
                  <div className="srow !border-t-0">
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
                              <span className={line.ok ? "" : "rt"}>
                                {line.head}
                              </span>
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
                </More>
              </>
            ) : null}
            {part === "start" ? (
              <>
                {atLogin !== null ? (
                  <>
                    <h2 className="set-h">{t("settings.start")}</h2>
                    <div className="srow">
                      <div>
                        <div className="l">{t("settings.atLogin")}</div>
                        <div className="h">
                          {t("settings.atLoginHint")}
                          {loginItem === "needs_approval" ? (
                            <span className="block rt">
                              {t("settings.atLoginOffInSystem")}
                            </span>
                          ) : null}
                        </div>
                      </div>
                      <div className="v">
                        {loginItem === "needs_approval" ? (
                          <button
                            type="button"
                            className="btn"
                            onClick={() => void commands.seogiOpenLoginItems()}
                          >
                            {t("permissions.openSettings")}
                          </button>
                        ) : null}
                        <input
                          type="checkbox"
                          role="switch"
                          className="tg"
                          checked={
                            loginItem === null || loginItem === "unknown"
                              ? atLogin
                              : loginItem === "on"
                          }
                          aria-label={t("settings.atLogin")}
                          onChange={(event) =>
                            void chooseAtLogin(event.target.checked)
                          }
                        />
                      </div>
                    </div>
                  </>
                ) : null}
                <Permissions firstRun={false} />
              </>
            ) : null}
          </div>
        </div>
      </main>
    </div>
  );
}

/** What is rarely changed in a group, folded under "더 보기". */
/** The events turned off on the home or from a reminder, each with a way
 *  to turn it back on. An event no longer in the calendars is named so. */
function MutedEvents({ uids }: { uids: string[] }) {
  const { t } = useTranslation(NS);
  const refreshSetup = useSeogi((s) => s.refreshSetup);
  const refreshToday = useSeogi((s) => s.refreshToday);
  const [titles, setTitles] = useState<Record<string, string>>({});
  const key = uids.join("|");

  useEffect(() => {
    let current = true;
    // The next three months of the calendars, to put a title to each id.
    void commands.seogiUpcoming(24 * 90).then((events) => {
      if (!current) return;
      const found: Record<string, string> = {};
      for (const event of events) found[event.uid] ??= event.title;
      setTitles(found);
    });
    return () => {
      current = false;
    };
  }, [key]);

  const unmute = async (uid: string) => {
    await commands.seogiMuteEvent(uid, false);
    await Promise.all([refreshSetup(), refreshToday()]);
  };

  return (
    <div className="srow">
      <div>
        <div className="l">{t("settings.mutedEvents")}</div>
        <div className="h">{t("settings.mutedEventsHint")}</div>
      </div>
      <div className="v" />
      <div className="full grid gap-1.5 text-[13.5px]">
        {uids.map((uid) => (
          <div key={uid} className="flex items-start gap-3">
            <span
              className={`flex-1 min-w-0 truncate${titles[uid] ? "" : " t3"}`}
            >
              {titles[uid] ?? t("settings.mutedGone")}
            </span>
            <button
              type="button"
              className="lnk"
              onClick={() => void unmute(uid)}
            >
              {t("settings.unmute")}
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

function More({ children }: { children: ReactNode }) {
  const { t } = useTranslation(NS);
  return (
    <details className="mt-6">
      <summary className="fold">
        <span>{t("settings.moreSettings")}</span>
        <span className="r">
          <RightIcon />
        </span>
      </summary>
      <div className="fold-body !pt-0">{children}</div>
    </details>
  );
}
