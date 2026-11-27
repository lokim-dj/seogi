// What the computer has to allow before Seogi can do its work, one row
// each: what it is for, whether it is allowed, and the one button that
// allows it. Settings shows every row; the first-run steps show a few rows
// each (`only`), under their own heading. Nothing here opens a window by
// itself: every system question comes from a press.

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { commands } from "@/bindings";
import { NS } from "../i18n";
import { useSeogi } from "../store";

export type Allowed = "checking" | "yes" | "no";

/** How often the rows look again while something still needs allowing:
 *  the user allows it in System Settings, and the row changes by itself. */
const LOOK_EVERY_MS = 2000;

/** Whether Seogi may type into other apps (macOS's Accessibility), looked
 *  at again while it is not allowed, so it changes by itself once the user
 *  allows it in System Settings. Elsewhere it stays "checking". */
export function useTypingPermission(mac: boolean): Allowed {
  const [typing, setTyping] = useState<Allowed>("checking");
  const looking = useRef(false);
  const before = useRef<Allowed>("checking");

  const look = useCallback(async () => {
    if (!mac || looking.current) return;
    looking.current = true;
    try {
      const typed = await checkAccessibilityPermission().catch(() => false);
      setTyping(typed ? "yes" : "no");
      // Just allowed: the dictation status starts dictation listening
      // (asked once, not on every look; it does more work).
      if (typed && before.current === "no") {
        void commands.seogiDictationStatus();
      }
      before.current = typed ? "yes" : "no";
    } finally {
      looking.current = false;
    }
  }, [mac]);

  useEffect(() => {
    void look();
  }, [look]);

  useEffect(() => {
    if (typing !== "no") return;
    const timer = window.setInterval(() => void look(), LOOK_EVERY_MS);
    return () => window.clearInterval(timer);
  }, [typing, look]);

  return typing;
}

/** The rows this component can show. */
export type PermissionRow =
  | "microphone"
  | "typing"
  | "calendar"
  | "notify"
  | "documents";

export default function Permissions({
  firstRun,
  only,
}: {
  firstRun: boolean;
  /** Only these rows, without the heading: the first run's steps give
   *  their own. All rows (and the heading) when left out. */
  only?: PermissionRow[];
}) {
  const { t } = useTranslation(NS);
  const os = platform();
  const mac = os === "macos";
  const windows = os === "windows";

  // Typing into other apps matters only while dictation is on; off, its
  // missing permission is told quietly, not in red.
  const dictating = useSeogi(
    (state) => state.config?.dictation.enabled ?? true,
  );
  const [microphone, setMicrophone] = useState<Allowed>("checking");
  const [micAsked, setMicAsked] = useState(false);
  const typing = useTypingPermission(mac);
  const [calendar, setCalendar] = useState("");
  const [notified, setNotified] = useState<"no" | "sent" | "failed">("no");
  const [calendarTrouble, setCalendarTrouble] = useState(false);
  // One look at a time: a slow answer is not asked again on top of itself.
  const looking = useRef(false);

  const look = useCallback(async () => {
    if (looking.current) return;
    looking.current = true;
    try {
      if (mac) {
        const mic = await checkMicrophonePermission().catch(() => false);
        setMicrophone(mic ? "yes" : "no");
        setCalendar(await commands.seogiMacCalendarAccess());
      } else if (windows) {
        const status = await commands.getWindowsMicrophonePermissionStatus();
        setMicrophone(
          !status.supported || status.overall_access !== "denied"
            ? "yes"
            : "no",
        );
      }
    } finally {
      looking.current = false;
    }
  }, [mac, windows]);

  useEffect(() => {
    void look();
  }, [look]);

  const waiting = microphone === "no" || typing === "no";
  useEffect(() => {
    if (!waiting) return;
    const timer = window.setInterval(() => void look(), LOOK_EVERY_MS);
    return () => window.clearInterval(timer);
  }, [waiting, look]);

  if (!mac && !windows) return null;
  const shows = (row: PermissionRow) => !only || only.includes(row);

  const askMicrophone = async () => {
    // macOS asks once; after that only System Settings can allow it.
    if (mac && !micAsked) {
      setMicAsked(true);
      try {
        await requestMicrophonePermission();
        // The system's question takes the focus away. Still in front a
        // moment later and still not allowed: it was refused before and
        // there is no question, so System Settings opens instead.
        window.setTimeout(() => {
          void checkMicrophonePermission()
            .catch(() => false)
            .then((allowed) => {
              if (!allowed && document.hasFocus())
                void commands.openMicrophonePrivacySettings();
            });
        }, 1000);
      } catch {
        await commands.openMicrophonePrivacySettings();
      }
    } else {
      await commands.openMicrophonePrivacySettings();
    }
    void look();
  };

  const askCalendar = async () => {
    const result = await commands.seogiMacCalendarRequest();
    setCalendarTrouble(result.status === "error");
    if (result.status === "ok") setCalendar(result.data);
  };

  const tryNotification = async () => {
    const result = await commands.seogiTryNotification(
      t("permissions.notifyTitle"),
      t("permissions.notifyBody"),
    );
    setNotified(result.status === "ok" ? "sent" : "failed");
  };

  const openLabel = t(
    windows ? "permissions.openWindows" : "permissions.openSettings",
  );

  /** The name, and whether it is needed, needed only for typing into
   *  other apps (dictation), or up to the user. */
  const name = (label: string, needed: boolean | "typing") => (
    <div className="l">
      {label}
      <span className="t3 text-[13px] ml-2">
        {t(
          needed === "typing"
            ? "permissions.forTyping"
            : needed
              ? "permissions.needed"
              : "permissions.optional",
        )}
      </span>
    </div>
  );

  /** "허락됨" quietly, or what is missing in red when it is needed. */
  const state = (allowed: Allowed, needed: boolean, off?: string) =>
    allowed === "checking" ? null : allowed === "yes" ? (
      <span className="t2">{t("permissions.allowed")}</span>
    ) : (
      <span className={needed ? "rt" : "t3"}>
        {off ?? t("permissions.notYet")}
      </span>
    );

  const calendarAllowed: Allowed =
    calendar === "" ? "checking" : calendar === "granted" ? "yes" : "no";
  const calendarBlocked = calendar === "denied" || calendar === "restricted";

  const rows = (
    <>
      {shows("microphone") ? (
        <div className="srow">
          <div>
            {name(t("permissions.microphone"), true)}
            <div className="h">{t("permissions.microphoneFor")}</div>
          </div>
          <div className="v">
            {state(microphone, true)}
            {microphone === "no" ? (
              <button
                type="button"
                className="btn"
                onClick={() => void askMicrophone()}
              >
                {mac && !micAsked ? t("permissions.allow") : openLabel}
              </button>
            ) : null}
          </div>
        </div>
      ) : null}

      {mac && shows("typing") ? (
        <div className="srow">
          <div>
            {name(t("permissions.typing"), "typing")}
            <div className="h">{t("permissions.typingFor")}</div>
          </div>
          <div className="v">
            {state(typing, dictating)}
            {typing === "no" ? (
              <button
                type="button"
                className="btn"
                onClick={() => void commands.seogiOpenTypingPermission()}
              >
                {openLabel}
              </button>
            ) : null}
          </div>
        </div>
      ) : null}

      {mac && shows("calendar") && calendar !== "unsupported" ? (
        <div className="srow">
          <div>
            {name(t("permissions.calendar"), false)}
            <div className="h">
              {t("permissions.calendarFor")}
              {calendarTrouble ? (
                <span className="block rt">{t("permissions.askFailed")}</span>
              ) : calendarBlocked ? (
                <span className="block">
                  {t("permissions.calendarBlocked")}
                </span>
              ) : null}
            </div>
          </div>
          <div className="v">
            {state(
              calendarAllowed,
              false,
              calendarBlocked ? t("permissions.off") : undefined,
            )}
            {calendar === "not_determined" ? (
              <button
                type="button"
                className="btn"
                onClick={() => void askCalendar()}
              >
                {t("permissions.allow")}
              </button>
            ) : null}
          </div>
        </div>
      ) : null}

      {mac && shows("notify") ? (
        <div className="srow">
          <div>
            {name(t("permissions.notify"), false)}
            <div className="h">
              {t("permissions.notifyFor")}
              {notified === "sent" ? (
                <span className="block">{t("permissions.notifySent")}</span>
              ) : notified === "failed" ? (
                <span className="block rt">{t("permissions.askFailed")}</span>
              ) : null}
            </div>
          </div>
          <div className="v">
            <button
              type="button"
              className="btn"
              onClick={() => void tryNotification()}
            >
              {t("permissions.notifyTry")}
            </button>
          </div>
        </div>
      ) : null}

      {mac && firstRun && shows("documents") ? (
        <div className="srow">
          <div>
            <div className="l">{t("permissions.documents")}</div>
            <div className="h">{t("permissions.documentsFor")}</div>
          </div>
          <div className="v" />
        </div>
      ) : null}
    </>
  );

  if (only) return <div>{rows}</div>;
  return (
    <section aria-labelledby="permissions-title">
      <h2 id="permissions-title" className="set-h">
        {t("permissions.title")}
      </h2>
      {rows}
    </section>
  );
}
