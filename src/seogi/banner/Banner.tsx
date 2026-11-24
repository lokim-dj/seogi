// What the banner shows. It asks the backend what there is to say, and
// tells it which button was pressed; the backend does the rest.

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, events, type Banner as BannerData } from "@/bindings";
import { NS } from "../i18n";
import { hm } from "../lib/format";

/** A reminder nobody answers goes away after this long. */
const STARTING_STAYS_MS = 5 * 60 * 1000;
const READY_STAYS_MS = 12 * 1000;

export default function Banner() {
  const { t } = useTranslation(NS);
  const [banner, setBanner] = useState<BannerData | null>(null);

  const refresh = useCallback(async () => {
    setBanner(await commands.seogiBanner());
  }, []);

  useEffect(() => {
    void refresh();
    const unlisten = events.bannerChanged.listen(() => void refresh());
    return () => {
      void unlisten.then((off) => off());
    };
  }, [refresh]);

  const act = useCallback((action: string) => {
    setBanner(null);
    void commands.seogiBannerAction(action);
  }, []);

  useEffect(() => {
    if (!banner) return;
    const timer = window.setTimeout(
      () => act("close"),
      banner.kind === "starting" ? STARTING_STAYS_MS : READY_STAYS_MS,
    );
    return () => window.clearTimeout(timer);
  }, [banner, act]);

  if (!banner) return null;

  if (banner.kind === "starting") {
    const minutes = Math.round((banner.event.start_ms - Date.now()) / 60000);
    return (
      <div className="p-5">
        <section
          aria-label={t("banner.startLabel")}
          className="box-border flex items-center gap-3 py-2.5 pr-2.5 pl-4 bg-s1 rounded-[12px] shadow-[0_10px_28px_var(--sh)]"
        >
          <span
            className="rec"
            style={{ width: 28, height: 28 }}
            aria-hidden="true"
          />
          <div className="flex-1 min-w-0">
            <div className="text-[14.5px] font-medium whitespace-nowrap overflow-hidden text-ellipsis">
              <span className="tab">{hm(banner.event.start_ms)}</span>{" "}
              {banner.event.title}
            </div>
            <div className="text-[13px] t3">
              {minutes >= 1
                ? t("banner.startingSoon", { n: minutes })
                : t("banner.startingNow")}
            </div>
          </div>
          <button
            type="button"
            onClick={() => act("record")}
            className="btn solid flex-none"
          >
            {t("banner.record")}
          </button>
          <button
            type="button"
            onClick={() => act("later")}
            className="btn flex-none"
          >
            {t("banner.later")}
          </button>
          <button
            type="button"
            onClick={() => act("mute")}
            className="lnk flex-none px-1 t3"
          >
            {t("banner.mute")}
          </button>
        </section>
      </div>
    );
  }

  return (
    <div className="p-5">
      <section
        aria-label={t("banner.readyLabel")}
        role="status"
        className="box-border px-4 py-3 bg-s1 rounded-[12px] shadow-[0_10px_28px_var(--sh)]"
      >
        <div className="text-[14.5px] font-medium leading-[1.35]">
          {banner.title}
        </div>
        <div className="mt-1 text-[13.5px] leading-normal t2">
          {t("banner.ready", { n: banner.actions })}
          {banner.to_check > 0 ? (
            <>
              {", "}
              <span className="rt whitespace-nowrap">
                {t("banner.readyCheck", { n: banner.to_check })}
              </span>
            </>
          ) : null}
        </div>
        <button type="button" onClick={() => act("open")} className="lnk mt-2">
          {t("banner.open")}
        </button>
      </section>
    </div>
  );
}
