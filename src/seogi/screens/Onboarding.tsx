// The first run, in three steps shown one at a time: who writes the
// minutes; what the computer must allow; and what is nice to have (the
// calendar, the folder phone recordings arrive in, dictation), which can be
// skipped and done later in Settings. Every choice lives here, so going
// back a step keeps it; the settings are written by the last press.

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { platform } from "@tauri-apps/plugin-os";
import { commands, type CalendarCheck, type SignIns } from "@/bindings";
import { NS } from "../i18n";
import { friendlyPath, keysLabel, keysShort, shortModel } from "../lib/format";
import { gigabytes, gigabytesShort, minutesToDownload } from "../lib/speech";
import {
  KEY_OWNERS,
  LOCAL,
  loginProvider,
  modeOf,
  type KeyOwner,
  type LoginTool,
  type Mode,
} from "../lib/summary";
import Permissions, { useTypingPermission } from "../components/Permissions";
import SignInRows from "../components/SignInRows";
import { useSeogi } from "../store";

const MODES: Mode[] = ["local", "login", "key"];
const STEPS = ["how", "needed", "nice"] as const;
type Step = 0 | 1 | 2;
/** Where the calendar is read from: the Mac's Calendar app or an address. */
type CalendarWay = "mac" | "address";
/** How the first run ends on step 3: skipping the nice-to-haves, or with
 *  everything on the screen. */
type Finish = "skip" | "all";

export default function Onboarding() {
  const { t } = useTranslation(NS);
  const mac = platform() === "macos";
  const config = useSeogi((s) => s.config);
  const readiness = useSeogi((s) => s.readiness);
  const downloads = useSeogi((s) => s.downloads);
  const downloadMinutesLeft = useSeogi((s) => s.downloadMinutesLeft);
  const downloadFailed = useSeogi((s) => s.downloadFailed);
  const downloadShort = useSeogi((s) => s.downloadShort);
  const downloadSpeechModel = useSeogi((s) => s.downloadSpeechModel);
  const saveConfig = useSeogi((s) => s.saveConfig);
  const refreshSetup = useSeogi((s) => s.refreshSetup);

  // What is already chosen (a setting carried over, or made for the user
  // beforehand) is what the table starts on.
  const [mode, setMode] = useState<Mode>(() =>
    config ? modeOf(config.summary) : "local",
  );
  const [tool, setTool] = useState<LoginTool>(() =>
    config?.summary.kind === "codex_cli" ? "codex" : "claude",
  );
  const [owner, setOwner] = useState<KeyOwner>("anthropic");
  const [key, setKey] = useState("");
  const [pulling, setPulling] = useState(false);
  const [url, setUrl] = useState("");
  const [checking, setChecking] = useState(false);
  const [calendar, setCalendar] = useState<CalendarCheck | null>(null);
  const [checkedUrl, setCheckedUrl] = useState("");
  const [showHow, setShowHow] = useState(false);
  const [inbox, setInbox] = useState("");
  const [drive, setDrive] = useState(0);
  const [made, setMade] = useState<string[]>([]);
  const [trouble, setTrouble] = useState("");
  const [busy, setBusy] = useState(false);
  // Seogi opens at login and dictation is on unless turned off; both are
  // said once, here, under what the computer has to allow.
  const [atLogin, setAtLogin] = useState(false);
  const [dictationKeys, setDictationKeys] = useState("");
  const [keyTrouble, setKeyTrouble] = useState(false);
  const [step, setStep] = useState<Step>(0);
  // "나중에 정하기" on step 1: the way of writing minutes is left as it is,
  // as deciding later always did, and the steps go on.
  const [methodLater, setMethodLater] = useState(false);
  // The Mac's Calendar app is the first choice where there is one; an
  // address is the only way elsewhere.
  const [way, setWay] = useState<CalendarWay>("mac");
  const [macAccess, setMacAccess] = useState("");
  const [macTrouble, setMacTrouble] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const headings = useRef<(HTMLHeadingElement | null)[]>([]);
  const moved = useRef(false);
  const typing = useTypingPermission(mac);
  // Typing into other apps matters only while dictation is on.
  const dictating = config?.dictation.enabled ?? true;

  useEffect(() => {
    void refreshSetup();
  }, [refreshSetup]);

  useEffect(() => {
    void commands.getAppSettings().then((result) => {
      if (result.status === "ok")
        setAtLogin(result.data.autostart_enabled ?? false);
    });
    void commands.seogiDictation().then((dictation) => {
      if (!dictation.enabled || !dictation.shortcut) return;
      const mac = platform() === "macos";
      const keys = mac
        ? keysShort(dictation.shortcut, true)
        : keysLabel(dictation.shortcut, false);
      setDictationKeys(keys.replace(/space/i, "Space"));
    });
    // Whether the key could be registered: dictation starts listening a
    // moment after the app starts, so this looks once that has happened.
    const timer = window.setTimeout(() => {
      void commands.seogiDictationStatus().then((result) => {
        if (result.status === "ok")
          setKeyTrouble(!!result.data.shortcut_trouble);
      });
    }, 3000);
    return () => window.clearTimeout(timer);
  }, []);

  // Whether the Mac's calendars may be read, looked at again on every step
  // (it may have been allowed in System Settings meanwhile).
  useEffect(() => {
    if (!mac) return;
    void commands.seogiMacCalendarAccess().then(setMacAccess);
  }, [mac, step]);

  // A new step starts at its top, with its heading read out.
  useEffect(() => {
    if (!moved.current) return;
    root.current?.parentElement?.scrollTo({ top: 0 });
    headings.current[step]?.focus({ preventScroll: true });
  }, [step]);

  // Whichever login is found is the one offered.
  useEffect(() => {
    if (readiness && !readiness.claude_cli && readiness.codex_cli) {
      setTool("codex");
    }
  }, [readiness]);

  // Where the two subscription tools stand. The first reading picks the
  // one already signed in; a sign-in made here picks the one just signed
  // in to.
  const [signIns, setSignIns] = useState<SignIns | null>(null);
  const signInsBefore = useRef<SignIns | null>(null);
  const followSignIns = useCallback((now: SignIns) => {
    const before = signInsBefore.current;
    const just = (name: LoginTool) =>
      now[name] === "signed_in" && before?.[name] !== "signed_in";
    if (before ? just("codex") : now.claude !== "signed_in" && just("codex"))
      setTool("codex");
    else if (before && just("claude")) setTool("claude");
    signInsBefore.current = now;
    setSignIns(now);
  }, []);
  useEffect(() => {
    void commands.seogiSignIns().then((result) => {
      if (result.status === "ok") followSignIns(result.data);
    });
  }, [followSignIns]);

  if (!config) return null;
  // Each person's own Google Drive: the accounts found on this computer,
  // or a folder of their choosing, or one in the home folder when there is
  // no Drive at all.
  const drives = readiness?.drives ?? [];
  const inboxPath =
    inbox || drives[drive]?.inbox || readiness?.local_inbox || "";
  const inboxExists =
    !!inbox ||
    made.includes(inboxPath) ||
    (drives.find((account) => account.inbox === inboxPath)?.inbox_exists ??
      false);

  const localNeed = () => {
    if (!readiness) return "";
    if (!readiness.ollama_running) return t("onboarding.need.localNoOllama");
    const has = readiness.ollama_models.some(
      (name) => name === "gemma3:12b" || name === "gemma3:12b:latest",
    );
    return has
      ? t("onboarding.need.localReady")
      : t("onboarding.need.localNoModel");
  };
  const localNeedsPull =
    !!readiness &&
    readiness.ollama_running &&
    !readiness.ollama_models.includes("gemma3:12b");
  const loginFound = signIns?.[tool] === "signed_in";

  const pull = async () => {
    setPulling(true);
    await saveConfig({ ...config, summary: LOCAL });
    const result = await commands.seogiPullSummaryModel();
    setPulling(false);
    if (result.status === "error") setTrouble(result.error);
    await refreshSetup();
  };

  // What the address in the field gave when it was read, and for which
  // address: pressing start straight after pasting must not save nothing.
  const check = async (): Promise<CalendarCheck | null> => {
    const source = url.trim();
    if (!source) {
      setCalendar(null);
      return null;
    }
    if (calendar && checkedUrl === source) return calendar;
    setChecking(true);
    const result = await commands.seogiCheckCalendar(source);
    setChecking(false);
    const checked = result.status === "ok" ? result.data : null;
    setCalendar(checked);
    setCheckedUrl(source);
    return checked;
  };

  const offerMac = mac && macAccess !== "unsupported";
  const calendarWay: CalendarWay = offerMac ? way : "address";
  const macBlocked = macAccess === "denied" || macAccess === "restricted";

  const askMac = async () => {
    const result = await commands.seogiMacCalendarRequest();
    setMacTrouble(result.status === "error");
    if (result.status === "ok") setMacAccess(result.data);
  };

  const go = (to: Step) => {
    moved.current = true;
    setTrouble("");
    setStep(to);
  };

  const makeInbox = async () => {
    const result = await commands.seogiWatchFolder(inboxPath);
    if (result.status === "error") setTrouble(result.error);
    else setMade((paths) => [...paths, inboxPath]);
    await refreshSetup();
  };

  const changeInbox = async () => {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === "string") setInbox(picked);
  };

  const start = async (finish: Finish) => {
    setBusy(true);
    setTrouble("");
    // The calendar address and the folder are kept only when step 3 is
    // finished, not skipped.
    const connect = finish === "all";
    const next = { ...config, set_up: true };
    if (!methodLater) {
      if (mode === "local") next.summary = LOCAL;
      if (mode === "login") next.summary = loginProvider(tool, readiness);
      if (mode === "key") {
        const chosen = KEY_OWNERS.find((item) => item.id === owner);
        if (chosen) next.summary = chosen.provider(chosen.model);
        if (key.trim()) {
          const saved = await commands.seogiSetKey(owner, key.trim());
          if (saved.status === "error") setTrouble(saved.error);
        }
      }
    }
    if (connect && calendarWay === "address") {
      const checked = url.trim() ? await check() : null;
      if (checked?.ok) next.calendar_urls = [checked.address || url.trim()];
    }
    const error = await saveConfig(next);
    if (error) setTrouble(error);
    if (connect && inboxPath) {
      const watched = await commands.seogiWatchFolder(inboxPath);
      if (watched.status === "error") setTrouble(watched.error);
    }
    // Transcribing needs its models whichever way it ends.
    if (readiness && !readiness.speech_model_ready) {
      void downloadSpeechModel(readiness.speech_model);
    }
    if (readiness && !readiness.diarizer_ready) {
      void commands.seogiDownloadDiarizer();
    }
    // Dictation's own model too; it uses the meetings' model until then.
    void commands.seogiDownloadDictationModel();
    await refreshSetup();
    setBusy(false);
  };

  const speechPercent = readiness
    ? Math.round(downloads[readiness.speech_model] ?? -1)
    : -1;
  const speechState = () => {
    if (!readiness) return { head: "", hint: "" };
    if (readiness.speech_model_ready && readiness.diarizer_ready) {
      return {
        head: t("onboarding.speechReady"),
        hint: t("onboarding.speechReadyHint"),
      };
    }
    const standIn = readiness.speech_model_stand_in;
    if (speechPercent >= 0) {
      const left = downloadMinutesLeft[readiness.speech_model];
      return {
        head: left
          ? t("onboarding.speechLoadingLeft", {
              percent: speechPercent,
              minutes: left,
            })
          : t("onboarding.speechLoading", { percent: speechPercent }),
        hint: standIn
          ? t("onboarding.speechStandIn", { model: shortModel(standIn) })
          : t("onboarding.speechLoadingHint"),
      };
    }
    if (downloadFailed.includes(readiness.speech_model)) {
      const short = downloadShort[readiness.speech_model];
      return {
        head:
          short !== undefined
            ? t("settings.noSpace", { gb: gigabytesShort(short) })
            : t("onboarding.speechFailed"),
        hint: standIn
          ? `${t("onboarding.speechStandIn", { model: shortModel(standIn) })} · ${t("onboarding.speechFailedHint")}`
          : t("onboarding.speechFailedHint"),
      };
    }
    const bytes = readiness.speech_model_ready
      ? 0
      : readiness.speech_model_bytes;
    return {
      head: t("onboarding.speechMissing"),
      hint:
        bytes > 0
          ? `${t("onboarding.speechMissingHint")} · ${t("onboarding.speechSize", { gb: gigabytes(bytes), minutes: minutesToDownload(bytes) })}`
          : t("onboarding.speechMissingHint"),
    };
  };
  const speech = speechState();

  const column = (name: Mode) =>
    `cell !py-2.5 !px-3 ${mode === name ? "!bg-s1" : ""}`;
  const state = "cell w-[200px] !py-2.5 !px-3 font-normal t2";

  const heading = (index: Step, text: string) => (
    <h1
      ref={(element) => {
        headings.current[index] = element;
      }}
      tabIndex={-1}
      className="a-h1 mb-1.5 outline-none"
    >
      {text}
    </h1>
  );
  const lead = (text: string) => (
    <p className="mt-0 mb-5 text-[14px] leading-[1.6] t3 text-pretty">{text}</p>
  );

  return (
    <div ref={root} className="min-h-full flex flex-col">
      <header className="a-bar !justify-start gap-4">
        <strong className="wm">{t("brand")}</strong>
        <span className="text-[13px] t3">{t("onboarding.once")}</span>
      </header>

      <main className="flex-1 pb-12">
        <div className="w-[min(860px,calc(100vw-32px))] mx-auto pt-6">
          <ol
            aria-label={t("onboarding.steps")}
            className="flex flex-wrap gap-x-6 gap-y-1 m-0 mb-4 p-0 list-none text-[13px] font-normal"
          >
            {STEPS.map((name, index) => (
              <li
                key={name}
                aria-current={step === index ? "step" : undefined}
                className={step === index ? "" : index < step ? "t2" : "t3"}
              >
                <span className="mr-1.5">{index + 1}</span>
                {t(`onboarding.step.${name}`)}
              </li>
            ))}
          </ol>

          <div hidden={step !== 0}>
            {heading(0, t("onboarding.title"))}
            {lead(t("onboarding.lead"))}
            <div className="overflow-x-auto">
              <table className="w-full min-w-[680px] border-collapse text-[13.5px]">
                <thead>
                  <tr className="text-left">
                    <td className="cell w-[108px]" />
                    {MODES.map((name) => (
                      <th
                        key={name}
                        scope="col"
                        className={`w-[30%] !py-2.5 !px-3 rounded-t-[9px] ${
                          mode === name ? "bg-s1" : ""
                        }`}
                      >
                        <label className="flex items-center gap-2 text-[15px] font-normal cursor-pointer">
                          <input
                            type="radio"
                            name="how"
                            checked={mode === name}
                            onChange={() => setMode(name)}
                            className="w-3.5 h-3.5 m-0 accent-[var(--red)]"
                          />
                          {t(`onboarding.${name}`)}
                        </label>
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {(["money", "speed", "content"] as const).map((row) => (
                    <tr key={row}>
                      <th
                        scope="row"
                        className="cell !py-2.5 !px-3 text-left font-normal t3"
                      >
                        {t(
                          `onboarding.row${row[0].toUpperCase()}${row.slice(1)}`,
                        )}
                      </th>
                      {MODES.map((name) => (
                        <td
                          key={name}
                          className={`${column(name)} ${
                            row === "money" && name === "local"
                              ? "font-normal"
                              : ""
                          }`}
                        >
                          {t(`onboarding.${row}.${name}`)}
                        </td>
                      ))}
                    </tr>
                  ))}
                  <tr>
                    <th
                      scope="row"
                      className="cell !py-2.5 !px-3 text-left font-normal t3"
                    >
                      {t("onboarding.rowNeed")}
                    </th>
                    <td className={column("local")}>
                      {localNeed()}
                      {localNeedsPull ? (
                        <div className="mt-2">
                          {pulling ? (
                            <span className="text-[13px] t3">
                              {t("onboarding.need.pulling")}
                            </span>
                          ) : (
                            <button
                              type="button"
                              className="key key-small"
                              onClick={() => void pull()}
                            >
                              {t("onboarding.need.pull")}
                            </button>
                          )}
                        </div>
                      ) : null}
                    </td>
                    <td className={column("login")}>
                      {loginFound
                        ? t("onboarding.need.loginReady", {
                            tool: tool === "claude" ? "Claude" : "ChatGPT",
                          })
                        : t("onboarding.need.loginMissing")}
                      {mode === "login" ? (
                        <SignInRows compact onChange={followSignIns} />
                      ) : null}
                      {mode === "login" ? (
                        <label className="flex items-center gap-2 mt-3 text-[15px]">
                          <span className="font-normal">
                            {t("onboarding.loginWith")}
                          </span>
                          <select
                            value={tool}
                            onChange={(event) =>
                              setTool(event.target.value as LoginTool)
                            }
                            className="field field-soft !w-auto"
                          >
                            <option value="claude">{"Claude"}</option>
                            <option value="codex">{"ChatGPT (Codex)"}</option>
                          </select>
                        </label>
                      ) : null}
                      {mode === "login" ? (
                        <div className="mt-2 text-[13px] t3">
                          {t("signIn.noPlan")}
                        </div>
                      ) : null}
                    </td>
                    <td className={column("key")}>
                      {t("onboarding.need.key")}
                      {mode === "key" ? (
                        <div className="flex flex-col gap-2 mt-2">
                          <select
                            aria-label={t("onboarding.keyFor")}
                            value={owner}
                            onChange={(event) =>
                              setOwner(event.target.value as KeyOwner)
                            }
                            className="field field-soft"
                          >
                            {KEY_OWNERS.map((item) => (
                              <option key={item.id} value={item.id}>
                                {item.label}
                              </option>
                            ))}
                          </select>
                          <input
                            type="password"
                            autoComplete="off"
                            value={key}
                            onChange={(event) => setKey(event.target.value)}
                            placeholder={t("onboarding.keyField")}
                            aria-label={t("onboarding.keyField")}
                            className="field field-soft"
                          />
                          {owner === "gemini" ? (
                            <span className="rt text-[13px]">
                              {t("onboarding.geminiWarn")}
                            </span>
                          ) : null}
                        </div>
                      ) : null}
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>

          <div hidden={step !== 1}>
            {heading(1, t("onboarding.neededTitle"))}
            {lead(t(mac ? "permissions.lead" : "onboarding.neededLeadMic"))}
            <Permissions
              firstRun
              only={["microphone", "notify", "documents"]}
            />
          </div>

          <div hidden={step !== 2}>
            {heading(2, t("onboarding.niceTitle"))}
            {lead(t("onboarding.niceLead"))}
            <div className="overflow-x-auto">
              <table
                aria-label={t("onboarding.links")}
                className="w-full min-w-[680px] border-collapse text-[13.5px]"
              >
                <tbody>
                  <tr>
                    <th
                      scope="row"
                      className="cell w-[108px] !py-2.5 !px-3 text-left font-normal t3"
                    >
                      {t("onboarding.calendar")}
                    </th>
                    <td className="cell !py-2.5 !px-3">
                      {offerMac ? (
                        <div
                          className={`seg ${calendarWay === "address" ? "mb-2" : ""}`}
                          role="radiogroup"
                          aria-label={t("onboarding.calendarWay")}
                        >
                          {(["mac", "address"] as const).map((name) => (
                            <button
                              key={name}
                              type="button"
                              role="radio"
                              aria-checked={calendarWay === name}
                              onClick={() => setWay(name)}
                            >
                              {t(
                                name === "mac"
                                  ? "onboarding.calendarMac"
                                  : "onboarding.calendarAddress",
                              )}
                            </button>
                          ))}
                        </div>
                      ) : null}
                      {calendarWay === "address" ? (
                        <div className="flex flex-wrap gap-2">
                          <input
                            type="url"
                            value={url}
                            onChange={(event) => {
                              setUrl(event.target.value);
                              setCalendar(null);
                            }}
                            onBlur={() => void check()}
                            aria-label={t("onboarding.calendarField")}
                            placeholder="https://calendar.google.com/calendar/ical/…/basic.ics"
                            className="field flex-[1_1_320px] min-w-0"
                          />
                          <button
                            type="button"
                            className="key"
                            onClick={() => setShowHow((on) => !on)}
                          >
                            {t("onboarding.calendarHow")}
                          </button>
                        </div>
                      ) : null}
                      <div className="mt-1.5 text-[13px] t3">
                        {calendarWay === "mac" ? (
                          <>
                            {t("permissions.calendarFor")}
                            {macTrouble ? (
                              <span className="block rt">
                                {t("permissions.askFailed")}
                              </span>
                            ) : macBlocked ? (
                              <span className="block">
                                {t("permissions.calendarBlocked")}
                              </span>
                            ) : macAccess === "granted" ? (
                              <span className="block">
                                {t("permissions.calendarPick")}
                              </span>
                            ) : null}
                          </>
                        ) : showHow ? (
                          t("onboarding.calendarHowText")
                        ) : (
                          t("onboarding.calendarHint")
                        )}
                      </div>
                    </td>
                    <td className={state}>
                      {calendarWay === "mac" ? (
                        macAccess === "granted" ? (
                          t("permissions.allowed")
                        ) : macBlocked ? (
                          t("permissions.off")
                        ) : macAccess === "not_determined" ? (
                          <button
                            type="button"
                            className="key key-small"
                            onClick={() => void askMac()}
                          >
                            {t("permissions.allow")}
                          </button>
                        ) : null
                      ) : (
                        <>
                          {checking
                            ? t("onboarding.calendarChecking")
                            : calendar === null
                              ? t("onboarding.calendarNone")
                              : calendar.ok
                                ? t("onboarding.calendarOk")
                                : t("onboarding.calendarBad")}
                          <div className="text-[13px] font-normal t3">
                            {checking
                              ? null
                              : calendar === null
                                ? t("onboarding.calendarNoneHint")
                                : calendar.ok
                                  ? t("onboarding.calendarOkHint", {
                                      n: calendar.today,
                                    })
                                  : t(
                                      `settings.calendarReason.${calendar.reason}`,
                                      {
                                        defaultValue: t(
                                          "onboarding.calendarBadHint",
                                        ),
                                      },
                                    )}
                          </div>
                        </>
                      )}
                    </td>
                  </tr>
                  <tr>
                    <th
                      scope="row"
                      className="cell !py-2.5 !px-3 text-left font-normal t3"
                    >
                      {t("onboarding.inbox")}
                    </th>
                    <td className="cell !py-2.5 !px-3">
                      <div className="flex flex-wrap gap-2">
                        {drives.length > 1 && !inbox ? (
                          <select
                            aria-label={t("onboarding.inboxAccount")}
                            value={drive}
                            onChange={(event) =>
                              setDrive(Number(event.target.value))
                            }
                            className="field !w-auto max-w-full"
                          >
                            {drives.map((account, index) => (
                              <option key={account.inbox} value={index}>
                                {account.account}
                              </option>
                            ))}
                          </select>
                        ) : null}
                        <div
                          title={inboxPath}
                          className="flex-[1_1_320px] min-w-0 h-8 box-border flex items-center px-0 border-0 border-b border-line t2 whitespace-nowrap overflow-hidden text-ellipsis"
                        >
                          {friendlyPath(inboxPath)}
                        </div>
                        <button
                          type="button"
                          className="key"
                          onClick={() => void changeInbox()}
                        >
                          {t("onboarding.inboxChange")}
                        </button>
                      </div>
                      <div className="mt-1.5 text-[13px] t3">
                        {drives.length === 0 && !inbox
                          ? t("onboarding.inboxNoDrive")
                          : t("onboarding.inboxHint")}
                      </div>
                    </td>
                    <td className={state}>
                      {inboxExists ? (
                        <>
                          {made.includes(inboxPath)
                            ? t("onboarding.inboxMade")
                            : t("onboarding.inboxFound")}
                          <div className="text-[13px] font-normal t3">
                            {t("onboarding.inboxFoundHint")}
                          </div>
                        </>
                      ) : (
                        <>
                          <button
                            type="button"
                            className="key key-small"
                            onClick={() => void makeInbox()}
                          >
                            {drives.length > 0
                              ? t("onboarding.inboxMake")
                              : t("onboarding.inboxMakeHere")}
                          </button>
                          <div className="mt-1.5 text-[13px] font-normal t3">
                            {t("onboarding.inboxNewHint")}
                          </div>
                        </>
                      )}
                    </td>
                  </tr>
                  <tr>
                    <th
                      scope="row"
                      className="cell !py-2.5 !px-3 text-left font-normal t3"
                    >
                      {t("onboarding.speech")}
                    </th>
                    <td className="cell !py-2.5 !px-3 text-[13px] t3">
                      {t("onboarding.speechHint")}
                    </td>
                    <td className={state}>
                      {speech.head}
                      <div className="text-[13px] font-normal t3">
                        {speech.hint}
                      </div>
                    </td>
                  </tr>
                  {dictationKeys || mac ? (
                    <tr>
                      <th
                        scope="row"
                        className="cell !py-2.5 !px-3 text-left font-normal t3"
                      >
                        {t("dictation.title")}
                      </th>
                      <td className="cell !py-2.5 !px-3 text-[13px] t3">
                        {dictationKeys
                          ? t("onboarding.dictation", { keys: dictationKeys })
                          : null}
                        {keyTrouble && dictationKeys ? (
                          <span className="block rt">
                            {t("onboarding.dictationKeyTrouble", {
                              keys: dictationKeys,
                            })}
                          </span>
                        ) : null}
                        {mac && typing === "no" ? (
                          <span className="block">
                            {t("onboarding.typingHint")}
                          </span>
                        ) : null}
                      </td>
                      <td className={state}>
                        {!mac || typing === "checking" ? null : typing ===
                          "yes" ? (
                          t("onboarding.typingOk")
                        ) : (
                          <>
                            <span className={dictating ? "rt" : ""}>
                              {t("onboarding.typingNo")}
                            </span>
                            <div className="mt-1.5">
                              <button
                                type="button"
                                className="key key-small"
                                onClick={() =>
                                  void commands.seogiOpenTypingPermission()
                                }
                              >
                                {t("permissions.openSettings")}
                              </button>
                            </div>
                          </>
                        )}
                      </td>
                    </tr>
                  ) : null}
                  {atLogin ? (
                    <tr>
                      <th
                        scope="row"
                        className="cell !py-2.5 !px-3 text-left font-normal t3"
                      >
                        {t("onboarding.atLoginLabel")}
                      </th>
                      <td
                        colSpan={2}
                        className="cell !py-2.5 !px-3 text-[13px] t3"
                      >
                        {t("onboarding.atLogin")}
                      </td>
                    </tr>
                  ) : null}
                </tbody>
              </table>
            </div>
          </div>

          {trouble ? (
            <p role="alert" className="rt mt-4 mb-0 text-[13.5px]">
              {t("failed", { reason: trouble })}
            </p>
          ) : null}
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2.5 mt-6">
            {step > 0 ? (
              <button
                type="button"
                disabled={busy}
                onClick={() => go((step - 1) as Step)}
                className="btn !h-9 !px-4"
              >
                {t("onboarding.back")}
              </button>
            ) : null}
            <span className="flex-1" />
            {step === 0 ? (
              <button
                type="button"
                disabled={busy}
                onClick={() => {
                  setMethodLater(true);
                  go(1);
                }}
                className="lnk"
              >
                {t("onboarding.later")}
              </button>
            ) : null}
            {step === 2 ? (
              <button
                type="button"
                disabled={busy}
                onClick={() => void start("skip")}
                className="lnk"
              >
                {t("onboarding.skip")}
              </button>
            ) : null}
            {step < 2 ? (
              <button
                type="button"
                onClick={() => {
                  // "다음" on step 1 means the way in the table is chosen.
                  if (step === 0) setMethodLater(false);
                  go((step + 1) as Step);
                }}
                data-step-next
                className="btn solid !h-9 !px-5"
              >
                {t("onboarding.next")}
              </button>
            ) : (
              <button
                type="button"
                disabled={busy}
                onClick={() => void start("all")}
                className="btn solid !h-9 !px-5"
              >
                {t("onboarding.start")}
              </button>
            )}
          </div>
        </div>
      </main>
    </div>
  );
}
