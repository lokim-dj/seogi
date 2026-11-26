// The first run: one table to choose who writes the minutes, and two rows
// to connect the calendar and the folder phone recordings arrive in.

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
import Permissions from "../components/Permissions";
import SignInRows from "../components/SignInRows";
import { useSeogi } from "../store";

const MODES: Mode[] = ["local", "login", "key"];

export default function Onboarding() {
  const { t } = useTranslation(NS);
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

  const start = async (decideLater: boolean) => {
    setBusy(true);
    setTrouble("");
    const next = { ...config, set_up: true };
    if (!decideLater) {
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
      const checked = url.trim() ? await check() : null;
      if (checked?.ok) next.calendar_urls = [checked.address || url.trim()];
    }
    const error = await saveConfig(next);
    if (error) setTrouble(error);
    if (!decideLater && inboxPath) {
      const watched = await commands.seogiWatchFolder(inboxPath);
      if (watched.status === "error") setTrouble(watched.error);
    }
    // Transcribing needs its models whichever way was chosen.
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
  const state = "cell w-[200px] !py-2.5 !px-3 font-medium t2";

  return (
    <div className="min-h-full flex flex-col">
      <header className="a-bar !justify-start gap-4">
        <strong className="wm">{t("brand")}</strong>
        <span className="text-[13px] t3">{t("onboarding.once")}</span>
      </header>

      <main className="flex-1 pb-12">
        <div className="w-[min(860px,calc(100vw-32px))] mx-auto pt-6">
          <h1 className="a-h1 mb-1.5">{t("onboarding.title")}</h1>
          <p className="mt-0 mb-5 text-[14px] leading-[1.6] t3 text-pretty">
            {t("onboarding.lead")}
          </p>

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
                      <label className="flex items-center gap-2 text-[15px] font-medium cursor-pointer">
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
                      className="cell !py-2.5 !px-3 text-left font-medium t3"
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
                            ? "font-medium"
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
                    className="cell !py-2.5 !px-3 text-left font-medium t3"
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
                        <span className="font-medium">
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

          <div className="overflow-x-auto mt-5">
            <table
              aria-label={t("onboarding.links")}
              className="w-full min-w-[680px] border-collapse text-[13.5px]"
            >
              <tbody>
                <tr>
                  <th
                    scope="row"
                    className="cell w-[108px] !py-2.5 !px-3 text-left font-medium t3"
                  >
                    {t("onboarding.calendar")}
                  </th>
                  <td className="cell !py-2.5 !px-3">
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
                    <div className="mt-1.5 text-[13px] t3">
                      {showHow
                        ? t("onboarding.calendarHowText")
                        : t("onboarding.calendarHint")}
                    </div>
                  </td>
                  <td className={state}>
                    {checking
                      ? t("onboarding.calendarChecking")
                      : calendar === null
                        ? t("onboarding.calendarNone")
                        : calendar.ok
                          ? t("onboarding.calendarOk")
                          : t("onboarding.calendarBad")}
                    <div className="text-[13px] font-medium t3">
                      {checking
                        ? null
                        : calendar === null
                          ? t("onboarding.calendarNoneHint")
                          : calendar.ok
                            ? t("onboarding.calendarOkHint", {
                                n: calendar.today,
                              })
                            : t(`settings.calendarReason.${calendar.reason}`, {
                                defaultValue: t("onboarding.calendarBadHint"),
                              })}
                    </div>
                  </td>
                </tr>
                <tr>
                  <th
                    scope="row"
                    className="cell !py-2.5 !px-3 text-left font-medium t3"
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
                        <div className="text-[13px] font-medium t3">
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
                        <div className="mt-1.5 text-[13px] font-medium t3">
                          {t("onboarding.inboxNewHint")}
                        </div>
                      </>
                    )}
                  </td>
                </tr>
                <tr>
                  <th
                    scope="row"
                    className="cell !py-2.5 !px-3 text-left font-medium t3"
                  >
                    {t("onboarding.speech")}
                  </th>
                  <td className="cell !py-2.5 !px-3 text-[13px] t3">
                    {t("onboarding.speechHint")}
                  </td>
                  <td className={state}>
                    {speech.head}
                    <div className="text-[13px] font-medium t3">
                      {speech.hint}
                    </div>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>

          <Permissions firstRun />

          {trouble ? (
            <p role="alert" className="rt mt-4 mb-0 text-[13.5px]">
              {t("failed", { reason: trouble })}
            </p>
          ) : null}
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2.5 mt-5">
            <span className="flex-[1_1_300px] text-[13px] t3">
              {atLogin ? (
                <span className="block">{t("onboarding.atLogin")}</span>
              ) : null}
              {dictationKeys ? (
                <span className="block">
                  {t("onboarding.dictation", { keys: dictationKeys })}
                  {keyTrouble ? (
                    <span className="block rt">
                      {t("onboarding.dictationKeyTrouble", {
                        keys: dictationKeys,
                      })}
                    </span>
                  ) : null}
                </span>
              ) : null}
            </span>
            <button
              type="button"
              disabled={busy}
              onClick={() => void start(true)}
              className="lnk"
            >
              {t("onboarding.later")}
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void start(false)}
              className="btn solid !h-9 !px-5"
            >
              {t("onboarding.start")}
            </button>
          </div>
        </div>
      </main>
    </div>
  );
}
