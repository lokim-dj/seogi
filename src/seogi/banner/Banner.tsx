// What the banner shows. It asks the backend what there is to say, and
// tells it which button was pressed; the backend does the rest.
//
// Put away by its close key, by Esc or by time, it slides out sideways
// first (or just fades, for someone who asked for less motion) and only
// then tells the backend, which hides the window.

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, events, type Banner as BannerData } from "@/bindings";
import { NS } from "../i18n";
import { hm } from "../lib/format";
import { CloseIcon } from "../components/icons";

/** A reminder nobody answers goes away after this long. */
const STARTING_STAYS_MS = 5 * 60 * 1000;
const READY_STAYS_MS = 12 * 1000;
/** How long the slide out takes; `.banner-box` in seogi.css matches it. */
const LEAVE_MS = 240;
const LEAVE_REDUCED_MS = 120;

/** Actions that open something else: the banner goes at once. */
const AT_ONCE = new Set(["open", "record"]);

function reducedMotion(): boolean {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
}

export default function Banner() {
  const { t } = useTranslation(NS);
  const [banner, setBanner] = useState<BannerData | null>(null);
  const [leaving, setLeaving] = useState(false);
  const leaveTimer = useRef<number | undefined>(undefined);

  const refresh = useCallback(async () => {
    // A new banner may have arrived while the old one was sliding out:
    // stop the slide before asking, so its late "close" cannot put the new
    // one away.
    window.clearTimeout(leaveTimer.current);
    leaveTimer.current = undefined;
    const next = await commands.seogiBanner();
    setLeaving(false);
    setBanner(next);
  }, []);

  useEffect(() => {
    void refresh();
    const unlisten = events.bannerChanged.listen(() => void refresh());
    return () => {
      void unlisten.then((off) => off());
      window.clearTimeout(leaveTimer.current);
    };
  }, [refresh]);

  const act = useCallback((action: string) => {
    if (leaveTimer.current !== undefined) return;
    const done = () => {
      leaveTimer.current = undefined;
      setBanner(null);
      setLeaving(false);
      void commands.seogiBannerAction(action);
    };
    if (AT_ONCE.has(action)) {
      done();
      return;
    }
    setLeaving(true);
    leaveTimer.current = window.setTimeout(
      done,
      reducedMotion() ? LEAVE_REDUCED_MS : LEAVE_MS,
    );
  }, []);

  useEffect(() => {
    if (!banner) return;
    const timer = window.setTimeout(
      () => act("close"),
      banner.kind === "starting" ? STARTING_STAYS_MS : READY_STAYS_MS,
    );
    // Esc reaches the banner only where its window can take the keyboard
    // (on Windows once it has been clicked); on macOS it is a panel that
    // never does, and there the close key is the way.
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") act("close");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("keydown", onKey);
    };
  }, [banner, act]);

  if (!banner) return null;

  const close = (
    <button
      type="button"
      onClick={() => act("close")}
      aria-label={t("banner.close")}
      title={t("banner.close")}
      className="banner-x"
    >
      <CloseIcon />
    </button>
  );

  if (banner.kind === "starting") {
    const minutes = Math.round((banner.event.start_ms - Date.now()) / 60000);
    return (
      <div className="p-3 overflow-hidden">
        <section
          aria-label={t("banner.startLabel")}
          className={`banner-box flex items-center gap-2.5 py-2 pr-1.5 pl-3${leaving ? " leaving" : ""}`}
        >
          <span
            className="rec"
            style={{ width: 22, height: 22 }}
            aria-hidden="true"
          />
          <div className="flex-1 min-w-0">
            <div className="text-[13px] font-normal leading-[1.35] whitespace-nowrap overflow-hidden text-ellipsis">
              <span className="tab">{hm(banner.event.start_ms)}</span>{" "}
              {banner.event.title}
            </div>
            <div className="text-[12px] leading-[1.35] t3">
              {minutes >= 1
                ? t("banner.startingSoon", { n: minutes })
                : t("banner.startingNow")}
            </div>
          </div>
          <button
            type="button"
            onClick={() => act("record")}
            className="btn solid banner-btn flex-none"
          >
            {t("banner.record")}
          </button>
          <button
            type="button"
            onClick={() => act("mute")}
            className="lnk banner-lnk flex-none px-1 t3"
          >
            {t("banner.mute")}
          </button>
          {close}
        </section>
      </div>
    );
  }

  return (
    <div className="p-3 overflow-hidden">
      <section
        aria-label={t("banner.readyLabel")}
        role="status"
        className={`banner-box py-[7px] pr-1.5 pl-3${leaving ? " leaving" : ""}`}
      >
        <div className="flex items-start gap-1">
          <div className="flex-1 min-w-0 pt-px text-[13px] font-normal leading-[1.35] whitespace-nowrap overflow-hidden text-ellipsis">
            {banner.title}
          </div>
          <div className="-my-[3px]">{close}</div>
        </div>
        <div className="pr-1.5 text-[12px] leading-[1.4] t2">
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
        <button
          type="button"
          onClick={() => act("open")}
          className="lnk banner-lnk mt-1 block leading-[1.35]"
        >
          {t("banner.open")}
        </button>
      </section>
    </div>
  );
}
