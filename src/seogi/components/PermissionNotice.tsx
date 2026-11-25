// Bands for a permission macOS took away, as after an update it did not
// recognize as the same app: dictation can no longer type (Accessibility),
// or the Calendar app can no longer be read. The backend looks again on its
// own and takes the band down the moment Seogi is allowed again.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, events, type Notice, type NoticeCode } from "@/bindings";
import { NS } from "../i18n";

const LOST = {
  CalendarPermissionLost: {
    text: "permissionLost.calendar",
    open: () => commands.seogiOpenCalendarPermission(),
  },
  TypingPermissionLost: {
    text: "permissionLost.typing",
    open: () => commands.seogiOpenTypingPermission(),
  },
} as const;

type Lost = keyof typeof LOST;

const isLost = (code: NoticeCode): code is Lost => code in LOST;

const lost = (notices: Notice[]): Lost[] =>
  notices.map((notice) => notice.code).filter(isLost);

export default function PermissionNotice() {
  const { t } = useTranslation(NS);
  const [shown, setShown] = useState<Lost[]>([]);

  useEffect(() => {
    let stop = false;
    commands
      .getActiveNotices()
      .then((notices) => {
        if (!stop) setShown(lost(notices));
      })
      .catch(() => {
        // Asked again with the next change.
      });
    const unlisten = events.noticesChanged.listen((event) =>
      setShown(lost(event.payload.notices)),
    );
    return () => {
      stop = true;
      void unlisten.then((off) => off());
    };
  }, []);

  return (
    <>
      {shown.map((code) => (
        <div
          key={code}
          role="alert"
          className="flex flex-wrap items-center gap-x-3 gap-y-1 mb-2 px-3 py-2 rounded-[9px] bg-s1 text-[13.5px]"
        >
          <span className="rt min-w-0 flex-1">{t(LOST[code].text)}</span>
          <button
            type="button"
            onClick={() => void LOST[code].open()}
            className="btn solid !h-[26px] ml-auto"
          >
            {t("permissionLost.open")}
          </button>
        </div>
      ))}
    </>
  );
}
