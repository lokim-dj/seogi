// Bands for a permission macOS took away, as after an update it did not
// recognize as the same app: dictation can no longer type (Accessibility),
// or the Calendar app can no longer be read. The backend looks again on its
// own and takes the band down the moment Seogi is allowed again. One more
// band says the Calendar app gave nothing although Seogi may read it: the
// connection inside this process went stale, and restarting Seogi fixes it.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { relaunch } from "@tauri-apps/plugin-process";
import { commands, events, type Notice } from "@/bindings";
import { NS } from "../i18n";

type Lost =
  | "CalendarPermissionLost"
  | "TypingPermissionLost"
  | "CalendarUnreadable";

const TEXT: Record<
  Lost,
  | "permissionLost.calendar"
  | "permissionLost.typing"
  | "permissionLost.calendarUnreadable"
> = {
  CalendarPermissionLost: "permissionLost.calendar",
  TypingPermissionLost: "permissionLost.typing",
  CalendarUnreadable: "permissionLost.calendarUnreadable",
};

const isLost = (notice: Notice): notice is Notice & { code: Lost } =>
  notice.code in TEXT;

// A calendar access macOS no longer knows about (right after a new
// signature Seogi may not be in its list) is asked for again; one it refused
// is turned back on in System Settings.
const asks = (notice: Notice) =>
  notice.code === "CalendarPermissionLost" &&
  notice.detail === "not_determined";

const fix = (notice: Notice) => {
  if (notice.code === "CalendarUnreadable") return relaunch();
  if (asks(notice)) return commands.seogiMacCalendarRequest();
  if (notice.code === "CalendarPermissionLost")
    return commands.seogiOpenCalendarPermission();
  return commands.seogiOpenTypingPermission();
};

const action = (notice: Notice) =>
  notice.code === "CalendarUnreadable"
    ? "permissionLost.restart"
    : asks(notice)
      ? "permissionLost.ask"
      : "permissionLost.open";

export default function PermissionNotice() {
  const { t } = useTranslation(NS);
  const [shown, setShown] = useState<(Notice & { code: Lost })[]>([]);

  useEffect(() => {
    let stop = false;
    commands
      .getActiveNotices()
      .then((notices) => {
        if (!stop) setShown(notices.filter(isLost));
      })
      .catch(() => {
        // Asked again with the next change.
      });
    const unlisten = events.noticesChanged.listen((event) =>
      setShown(event.payload.notices.filter(isLost)),
    );
    return () => {
      stop = true;
      void unlisten.then((off) => off());
    };
  }, []);

  return (
    <>
      {shown.map((notice) => (
        <div
          key={notice.code}
          role="alert"
          className="flex flex-wrap items-center gap-x-3 gap-y-1 mb-2 px-3 py-2 rounded-[9px] bg-s1 text-[13.5px]"
        >
          <span className="rt min-w-0 flex-1">{t(TEXT[notice.code])}</span>
          <button
            type="button"
            onClick={() => void fix(notice)}
            className="btn solid !h-[26px] ml-auto"
          >
            {t(action(notice))}
          </button>
        </div>
      ))}
    </>
  );
}
