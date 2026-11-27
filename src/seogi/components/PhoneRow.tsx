// The phone and Drive: one settings row to connect Google, so the
// recordings the phone's Seogi puts in the user's Drive come in here by
// themselves, and the minutes made here go into the same Drive folder for
// the phone. It says which account is connected, when the folder was last
// looked at, and offers "지금 가져오기". Once connected, two switches say
// whether this computer's own meetings go up and their recordings with
// them, and "지난 회의 올리기" puts up the meetings done before, saying how
// many and how much first. While the browser sign-in is open, or a look or
// an upload is under way, the row reads the backend again every few
// seconds.

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type Backlog,
  type Config,
  type DriveStatus,
  type PhoneStatus,
} from "@/bindings";
import { NS } from "../i18n";
import { hm } from "../lib/format";

/** How often the row reads again while something is under way. */
const BUSY_EVERY_MS = 1500;
/** And otherwise, for the time of the last look. */
const IDLE_EVERY_MS = 15_000;

/** Codes the backend gives for a sign-in that did not finish. */
const CONNECT_TROUBLE: Record<string, string> = {
  cancelled: "",
  already_connecting: "",
  denied: "settings.phoneDenied",
  timed_out: "settings.phoneTimedOut",
  unavailable: "settings.phoneUnavailable",
};

/** Bytes as the row says them: `1.2` (GB), at least `0.1`. */
export function gigabytes(bytes: number): string {
  return Math.max(0.1, bytes / 1024 ** 3).toFixed(1);
}

export default function PhoneRow({
  onNote,
  config,
  onSave,
}: {
  onNote: (text: string) => void;
  config: Config;
  onSave: (next: Config) => Promise<void>;
}) {
  const { t } = useTranslation(NS);
  const [status, setStatus] = useState<PhoneStatus | null>(null);
  const [drive, setDrive] = useState<DriveStatus | null>(null);
  const [backlog, setBacklog] = useState<Backlog | null>(null);
  const [trouble, setTrouble] = useState("");

  const look = useCallback(async () => {
    const [phone, uploads] = await Promise.all([
      commands.seogiPhoneStatus(),
      commands.seogiDriveStatus(),
    ]);
    setStatus(phone);
    setDrive(uploads);
  }, []);

  const connected = !!status?.connected;
  const own = config.drive_upload_own;
  const audio = config.drive_upload_audio;
  const waiting = drive?.waiting ?? 0;
  // What "지난 회의 올리기" would put up, read again when what decides it
  // changes.
  useEffect(() => {
    if (!connected) {
      setBacklog(null);
      return;
    }
    void commands.seogiDriveBacklog().then((result) => {
      if (result.status === "ok") setBacklog(result.data);
    });
  }, [connected, own, audio, waiting]);

  const busy =
    !!status && (status.connecting || status.pulling || !!drive?.uploading);
  useEffect(() => {
    void look();
    const timer = window.setInterval(
      () => void look(),
      busy ? BUSY_EVERY_MS : IDLE_EVERY_MS,
    );
    return () => window.clearInterval(timer);
  }, [look, busy]);

  const connect = async () => {
    setTrouble("");
    // The sign-in waits for the browser; show it as under way at once.
    setStatus((now) => (now ? { ...now, connecting: true } : now));
    const result = await commands.seogiPhoneConnect();
    if (result.status === "ok") {
      setStatus(result.data);
      onNote(t("settings.phoneConnected"));
    } else {
      // Anything else (Google or the browser in trouble) gets one plain
      // line; the details are in the log.
      const known =
        CONNECT_TROUBLE[result.error] ?? "settings.phoneConnectFailed";
      setTrouble(known ? t(known) : "");
      await look();
    }
  };

  const disconnect = async () => {
    const result = await commands.seogiPhoneDisconnect();
    if (result.status === "ok") setStatus(result.data);
    else onNote(t("failed", { reason: result.error }));
  };

  const uploadBacklog = async () => {
    const result = await commands.seogiDriveUploadBacklog();
    if (result.status === "ok") {
      onNote(t("settings.driveBacklogStarted", { n: result.data }));
      setBacklog({ meetings: 0, bytes: 0 });
      await look();
    } else {
      onNote(t("failed", { reason: result.error }));
    }
  };

  const pullNow = async () => {
    await commands.seogiPhoneLookNow();
    setStatus((now) => (now ? { ...now, pulling: true } : now));
    window.setTimeout(() => void look(), 400);
  };

  if (!status) return null;

  const lookLine = status.pulling
    ? t("settings.phoneLooking")
    : status.last_look_ms > 0
      ? t("settings.phoneLooked", { time: hm(status.last_look_ms) })
      : t("settings.phoneNotYet");

  return (
    <>
      <div className="srow">
        <div>
          <div className="l">{t("settings.phone")}</div>
          <div className="h">{t("settings.phoneHint")}</div>
        </div>
        <div className="v">
          {!status.available ? (
            <span className="t3">{t("settings.phoneUnavailableShort")}</span>
          ) : status.connecting ? (
            <button
              type="button"
              className="lnk"
              onClick={() => void commands.seogiPhoneCancelConnect()}
            >
              {t("settings.phoneCancel")}
            </button>
          ) : status.connected ? (
            <>
              <span
                className="t2 truncate max-w-[220px]"
                title={status.account}
              >
                {status.account || t("settings.phoneConnectedShort")}
              </span>
              <button
                type="button"
                className="lnk"
                onClick={() => void disconnect()}
              >
                {t("settings.phoneDisconnect")}
              </button>
            </>
          ) : (
            <button
              type="button"
              className="btn"
              onClick={() => void connect()}
            >
              {status.needs_reconnect
                ? t("settings.phoneReconnect")
                : t("settings.phoneConnect")}
            </button>
          )}
        </div>
        {!status.available ? (
          <p className="full m-0 text-[13.5px] t3">
            {t("settings.phoneUnavailable")}
          </p>
        ) : status.connecting ? (
          <p className="full m-0 text-[13.5px] t2">
            {t("settings.phoneWaiting")}
          </p>
        ) : status.needs_reconnect ? (
          <p className="full m-0 text-[13.5px] rt">
            {t("settings.phoneLost", { account: status.account })}
          </p>
        ) : status.connected ? (
          <div className="full grid gap-1 text-[13.5px]">
            <div className="flex items-center gap-3">
              <span className="flex-1 min-w-0 t3">
                {lookLine}
                {status.brought_in > 0
                  ? ` · ${t("settings.phoneBrought", { n: status.brought_in })}`
                  : ""}
              </span>
              <button
                type="button"
                className="lnk"
                disabled={status.pulling}
                onClick={() => void pullNow()}
              >
                {t("settings.phonePullNow")}
              </button>
            </div>
            {status.trouble ? (
              <p className="m-0 rt">
                {t(
                  status.trouble === "offline"
                    ? "settings.phoneOffline"
                    : status.trouble === "local"
                      ? "settings.phoneLocal"
                      : "settings.phoneDrive",
                )}
              </p>
            ) : null}
          </div>
        ) : trouble ? (
          <p className="full m-0 text-[13.5px] rt">{trouble}</p>
        ) : null}
      </div>
      {connected ? (
        <>
          <div className="srow">
            <div>
              <div className="l">{t("settings.driveOwn")}</div>
              <div className="h">{t("settings.driveOwnHint")}</div>
            </div>
            <div className="v">
              <input
                type="checkbox"
                role="switch"
                className="tg"
                checked={own}
                aria-label={t("settings.driveOwn")}
                onChange={(event) =>
                  void onSave({
                    ...config,
                    drive_upload_own: event.target.checked,
                  })
                }
              />
            </div>
            {drive &&
            (drive.uploading ||
              waiting > 0 ||
              drive.audio_waiting > 0 ||
              drive.trouble) ? (
              <div className="full grid gap-1 text-[13.5px]">
                {drive.uploading || waiting > 0 ? (
                  <span className="t3">
                    {drive.uploading
                      ? t("settings.driveUploading")
                      : t("settings.driveWaiting", { n: waiting })}
                  </span>
                ) : null}
                {drive.audio_waiting > 0 ? (
                  <p className="m-0 rt">
                    {t("settings.driveAudioWaiting", {
                      n: drive.audio_waiting,
                    })}
                  </p>
                ) : null}
                {drive.trouble ? (
                  <p className="m-0 rt">
                    {t(
                      drive.trouble === "offline"
                        ? "settings.driveOffline"
                        : "settings.driveTrouble",
                    )}
                  </p>
                ) : null}
              </div>
            ) : null}
          </div>
          {own ? (
            <div className="srow">
              <div>
                <div className="l">{t("settings.driveAudio")}</div>
                <div className="h">{t("settings.driveAudioHint")}</div>
              </div>
              <div className="v">
                <input
                  type="checkbox"
                  role="switch"
                  className="tg"
                  checked={audio}
                  aria-label={t("settings.driveAudio")}
                  onChange={(event) =>
                    void onSave({
                      ...config,
                      drive_upload_audio: event.target.checked,
                    })
                  }
                />
              </div>
            </div>
          ) : null}
          {backlog && backlog.meetings > 0 ? (
            <div className="srow">
              <div>
                <div className="l">{t("settings.driveBacklog")}</div>
                <div className="h">
                  {t("settings.driveBacklogHint", {
                    n: backlog.meetings,
                    gb: gigabytes(backlog.bytes),
                  })}
                </div>
              </div>
              <div className="v">
                <button
                  type="button"
                  className="btn"
                  onClick={() => void uploadBacklog()}
                >
                  {t("settings.driveBacklogGo", { n: backlog.meetings })}
                </button>
              </div>
            </div>
          ) : null}
        </>
      ) : null}
    </>
  );
}
