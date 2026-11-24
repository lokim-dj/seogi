// Small pieces every screen uses.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { NS } from "../i18n";
import { clock } from "../lib/format";

export function PlayIcon({ size = 9, color = "currentColor" }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={color}
      aria-hidden="true"
    >
      <path d="M5 3l16 9-16 9z" />
    </svg>
  );
}

export function FlagIcon({ size = 18 }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.4"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M5 21V4M5 4h13l-3 4.5 3 4.5H5" />
    </svg>
  );
}

/** A timestamp that plays the recording from that moment when pressed. */
export function TimeKey({
  ms,
  onListen,
  active = false,
  icon = true,
  className = "",
}: {
  ms: number;
  onListen: (ms: number) => void;
  active?: boolean;
  icon?: boolean;
  className?: string;
}) {
  const { t } = useTranslation(NS);
  return (
    <button
      type="button"
      className={`tkey ${active ? "tkey-on" : ""} ${className}`}
      aria-label={t("home.listenFrom", { time: clock(ms) })}
      onClick={(event) => {
        event.stopPropagation();
        onListen(ms);
      }}
    >
      {icon ? <PlayIcon /> : null}
      {clock(ms)}
    </button>
  );
}

/** The time now, refreshed often enough for a clock that shows minutes. */
export function useNow(everyMs = 5000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), everyMs);
    return () => window.clearInterval(timer);
  }, [everyMs]);
  return now;
}

/** A length of time in words: 52분, 1시간 21분. */
export function useLength(): (ms: number) => string {
  const { t } = useTranslation(NS);
  return (ms: number) => {
    const minutes = Math.round(ms / 60000);
    if (ms < 60000) return t("length.seconds", { s: Math.round(ms / 1000) });
    if (minutes < 60) return t("length.minutes", { m: minutes });
    const h = Math.floor(minutes / 60);
    const m = minutes % 60;
    return m === 0
      ? t("length.hours", { h })
      : t("length.hoursMinutes", { h, m });
  };
}

/** A speaker's name as shown: one the transcript could not name is numbered. */
export function useSpeakerName(): (name: string) => string {
  const { t } = useTranslation(NS);
  return (name: string) => {
    const unnamed = /^화자 (\d+)$/.exec(name);
    return unnamed ? t("speaker", { n: unnamed[1] }) : name;
  };
}

export function isUnnamed(name: string): boolean {
  return /^화자 \d+$/.test(name);
}
