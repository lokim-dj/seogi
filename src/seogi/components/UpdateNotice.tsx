// A new version, offered in the band: found quietly when the app starts and
// every few hours, installed only when the user presses the key, and never
// while a recording or its minutes are under way.

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { NS } from "../i18n";
import { useSeogi } from "../store";

const LOOK_EVERY_MS = 6 * 60 * 60 * 1000;

/** The first lines of the release notes that say what changed. */
function changes(body: string | undefined): string {
  return (body ?? "")
    .split("\n")
    .filter((line) => !line.trimStart().startsWith("#"))
    .map((line) =>
      line
        .replace(/^[-*\s]+/, "")
        .replace(/\*\*/g, "")
        .trim(),
    )
    .filter((line) => line.length > 0)
    .slice(0, 2)
    .join(" ");
}

export default function UpdateNotice() {
  const { t } = useTranslation(NS);
  const recording = useSeogi((s) => s.recording);
  const meetings = useSeogi((s) => s.meetings);
  const [update, setUpdate] = useState<Update | null>(null);
  const [percent, setPercent] = useState(-1);
  const [failed, setFailed] = useState(false);
  const installing = useRef(false);

  useEffect(() => {
    let stop = false;
    const look = async () => {
      if (installing.current) return;
      try {
        const found = await check();
        if (!stop) setUpdate(found);
      } catch {
        // No connection, or no release yet: looked at again later.
      }
    };
    void look();
    const timer = window.setInterval(() => void look(), LOOK_EVERY_MS);
    return () => {
      stop = true;
      window.clearInterval(timer);
    };
  }, []);

  if (!update) return null;

  const busy =
    recording !== null ||
    meetings.some(
      (meeting) =>
        meeting.status === "recording" || meeting.status === "working",
    );

  const install = async () => {
    if (busy || installing.current) return;
    installing.current = true;
    setFailed(false);
    setPercent(0);
    let total = 0;
    let received = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          received += event.data.chunkLength;
          if (total > 0) setPercent(Math.round((received / total) * 100));
        }
        if (event.event === "Finished") setPercent(100);
      });
      await relaunch();
    } catch {
      installing.current = false;
      setPercent(-1);
      setFailed(true);
    }
  };

  const about = changes(update.body);
  return (
    <div
      role="status"
      className="flex flex-wrap items-center gap-x-3 gap-y-1 mb-2 px-3 py-2 rounded-[9px] bg-s1 text-[13.5px]"
    >
      <b>{t("update.available", { version: update.version })}</b>
      {about ? (
        <span className="t3 min-w-0 flex-1 truncate">{about}</span>
      ) : null}
      <span className="ml-auto flex items-center gap-3">
        {failed ? <span className="rt">{t("update.failed")}</span> : null}
        {busy && percent < 0 ? (
          <span className="t3">{t("update.busy")}</span>
        ) : null}
        <button
          type="button"
          disabled={busy || percent >= 0}
          onClick={() => void install()}
          className="btn solid !h-[26px]"
        >
          {percent >= 100
            ? t("update.restarting")
            : percent >= 0
              ? t("update.downloading", { percent })
              : t("update.install")}
        </button>
      </span>
    </div>
  );
}
