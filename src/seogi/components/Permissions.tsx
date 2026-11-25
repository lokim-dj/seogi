// What the computer has to allow before Seogi can do its work, one row
// each: what it is for, whether it is allowed, and the one button that
// allows it. The first-run screen shows it so a new user can allow
// everything in one place; Settings shows the same rows for later. Nothing
// here opens a window by itself: every system question comes from a press.

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkMicrophonePermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { commands } from "@/bindings";
import { NS } from "../i18n";

type Allowed = "checking" | "yes" | "no";

/** How often the rows look again while something still needs allowing:
 *  the user allows it in System Settings, and the row changes by itself. */
const LOOK_EVERY_MS = 2000;

export default function Permissions({ firstRun }: { firstRun: boolean }) {
  const { t } = useTranslation(NS);
  const os = platform();
  const mac = os === "macos";
  const windows = os === "windows";

  const [microphone, setMicrophone] = useState<Allowed>("checking");
  const [micAsked, setMicAsked] = useState(false);
  const [typing, setTyping] = useState<Allowed>("checking");
  const [calendar, setCalendar] = useState("");
  const [notified, setNotified] = useState(false);

  const look = useCallback(async () => {
    if (mac) {
      try {
        setMicrophone((await checkMicrophonePermission()) ? "yes" : "no");
      } catch {
        setMicrophone("no");
      }
      // The same look the dictation settings use; it also starts dictation
      // listening as soon as typing is allowed.
      const status = await commands.seogiDictationStatus();
      if (status.status === "ok")
        setTyping(status.data.permitted ? "yes" : "no");
      setCalendar(await commands.seogiMacCalendarAccess());
    } else if (windows) {
      const status = await commands.getWindowsMicrophonePermissionStatus();
      setMicrophone(
        !status.supported || status.overall_access !== "denied" ? "yes" : "no",
      );
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
    setCalendar(result.status === "ok" ? result.data : "denied");
  };

  const tryNotification = async () => {
    await commands.seogiTryNotification(
      t("permissions.notifyTitle"),
      t("permissions.notifyBody"),
    );
    setNotified(true);
  };

  const openLabel = t(
    windows ? "permissions.openWindows" : "permissions.openSettings",
  );

  /** The name, and whether it is needed or up to the user. */
  const name = (label: string, needed: boolean) => (
    <div className="l">
      {label}
      <span className="t3 text-[13px] ml-2">
        {t(needed ? "permissions.needed" : "permissions.optional")}
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
              {name(t("permissions.typing"), true)}
              <div className="h">{t("permissions.typingFor")}</div>
            </div>
            <div className="v">
              {state(typing, true)}
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
                  {calendarBlocked ? (
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
                {notified ? (
                  <span className="block">{t("permissions.notifySent")}</span>
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
