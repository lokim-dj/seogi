// The meetings' speech model is not on this computer while another stands
// in for it (after an update that changed the default, or a download that
// failed): offered in the band, downloaded only when the user presses the
// key. "나중에" hides it until the app is started again.

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { NS } from "../i18n";
import { shortModel } from "../lib/format";
import { gigabytes, minutesToDownload } from "../lib/speech";
import { useSeogi } from "../store";

export default function SpeechModelNotice() {
  const { t } = useTranslation(NS);
  const readiness = useSeogi((s) => s.readiness);
  const config = useSeogi((s) => s.config);
  const downloads = useSeogi((s) => s.downloads);
  const minutesLeft = useSeogi((s) => s.downloadMinutesLeft);
  const failedIds = useSeogi((s) => s.downloadFailed);
  const [later, setLater] = useState(false);

  if (
    later ||
    !readiness ||
    !config?.set_up ||
    readiness.speech_model_ready ||
    !readiness.speech_model_stand_in
  ) {
    return null;
  }
  const model = readiness.speech_model;
  const percent = Math.round(downloads[model] ?? -1);
  const left = minutesLeft[model];
  const failed = failedIds.includes(model);
  const bytes = readiness.speech_model_bytes;

  return (
    <div
      role="status"
      className="flex flex-wrap items-center gap-x-3 gap-y-1 mb-2 px-3 py-2 rounded-[9px] bg-s1 text-[13.5px]"
    >
      <b>{t("speechNotice.title", { model: shortModel(model) })}</b>
      <span className="t3 basis-full order-last">
        {t("speechNotice.standIn", {
          model: shortModel(readiness.speech_model_stand_in),
        })}
        {bytes > 0 && percent < 0
          ? ` · ${t("settings.speechSize", {
              gb: gigabytes(bytes),
              minutes: minutesToDownload(bytes),
            })}`
          : null}
      </span>
      <span className="ml-auto flex items-center gap-3">
        {failed ? (
          <span className="rt">{t("settings.downloadFailed")}</span>
        ) : null}
        {percent >= 0 ? (
          <span className="t2">
            {left
              ? t("settings.downloadingLeft", { percent, minutes: left })
              : t("settings.downloading", { percent })}
          </span>
        ) : (
          <>
            <button
              type="button"
              className="lnk"
              onClick={() => setLater(true)}
            >
              {t("speechNotice.later")}
            </button>
            <button
              type="button"
              onClick={() => void commands.downloadModel(model)}
              className="btn solid !h-[26px]"
            >
              {t(failed ? "settings.downloadAgain" : "settings.download")}
            </button>
          </>
        )}
      </span>
    </div>
  );
}
