// The band that says dictation stopped because macOS no longer lets Seogi
// type (Accessibility), as after an update macOS did not recognize. The
// backend looks every few seconds and takes the band down, and the shortcut
// back, the moment Seogi is allowed again.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, events, type Notice } from "@/bindings";
import { NS } from "../i18n";

const lost = (notices: Notice[]) =>
  notices.some((notice) => notice.code === "TypingPermissionLost");

export default function PermissionNotice() {
  const { t } = useTranslation(NS);
  const [shown, setShown] = useState(false);

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

  if (!shown) return null;
  return (
    <div
      role="alert"
      className="flex flex-wrap items-center gap-x-3 gap-y-1 mb-2 px-3 py-2 rounded-[9px] bg-s1 text-[13.5px]"
    >
      <span className="rt min-w-0 flex-1">{t("permissionLost.text")}</span>
      <button
        type="button"
        onClick={() => void commands.seogiOpenTypingPermission()}
        className="btn solid !h-[26px] ml-auto"
      >
        {t("permissionLost.open")}
      </button>
    </div>
  );
}
