// What the computer has to allow before Seogi can do its work, one row
// each: what it is for, whether it is allowed, and the one button that
// allows it. The first-run screen shows it so a new user can allow
// everything in one place; Settings shows the same rows for later. Nothing
// here opens a window by itself: every system question comes from a press.

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

type Allowed = "checking" | "yes" | "no";

/** How often the rows look again while something still needs allowing:
 *  the user allows it in System Settings, and the row changes by itself. */
const LOOK_EVERY_MS = 2000;

export default function Permissions({ firstRun }: { firstRun: boolean }) {
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
  const [typing, setTyping] = useState<Allowed>("checking");
  const [calendar, setCalendar] = useState("");
  const [notified, setNotified] = useState<"no" | "sent" | "failed">("no");
  const [calendarTrouble, setCalendarTrouble] = useState(false);
  // One look at a time: a slow answer is not asked again on top of itself.
  const looking = useRef(false);
  const typingBefore = useRef<Allowed>("checking");

  const look = useCallback(async () => {
    if (looking.current) return;
    looking.current = true;
    try {
      if (mac) {
        const [mic, typed] = await Promise.all([
          checkMicrophonePermission().catch(() => false),
          checkAccessibilityPermission().catch(() => false),
        ]);
        setMicrophone(mic ? "yes" : "no");
        setTyping(typed ? "yes" : "no");
        // Just allowed: the dictation status starts dictation listening
        // (asked once, not on every look; it does more work).
        if (typed && typingBefore.current === "no") {
          void commands.seogiDictationStatus();
        }
        typingBefore.current = typed ? "yes" : "no";
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

  return (
    <section aria-labelledby="permissions-title">
      <h2 id="permissions-title" className="set-h">
        {t("permissions.title")}
      </h2>
      {firstRun ? (
        <p className="m-0 mb-1 text-[13px] t3">{t("permissions.lead")}</p>
      ) : null}

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

      {mac ? (
        <>
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

          {calendar !== "unsupported" ? (
            <div className="srow">
              <div>
                {name(t("permissions.calendar"), false)}
                <div className="h">
                  {t("permissions.calendarFor")}
                  {calendarTrouble ? (
                    <span className="block rt">
                      {t("permissions.askFailed")}
                    </span>
                  ) : calendarBlocked ? (
                    <span className="block">
                      {t("permissions.calendarBlocked")}
                    </span>
                  ) : calendar === "granted" && firstRun ? (
                    <span className="block">
                      {t("permissions.calendarPick")}
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

          {firstRun ? (
            <div className="srow">
              <div>
                <div className="l">{t("permissions.documents")}</div>
                <div className="h">{t("permissions.documentsFor")}</div>
              </div>
              <div className="v" />
            </div>
          ) : null}
        </>
      ) : null}
    </section>
  );
}
