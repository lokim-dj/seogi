// The meetings' speech model is not on this computer while another stands
// in for it (after an update that changed the default, a download that
// failed, or a chosen model that went missing): offered in the band,
// downloaded only when the user presses the key. "나중에" puts it off until
// the app is started again.

import { useTranslation } from "react-i18next";
import { NS } from "../i18n";
import { shortModel } from "../lib/format";
import { gigabytes, minutesToDownload } from "../lib/speech";
import { useSeogi } from "../store";
import SpeechDownload from "./SpeechDownload";

export default function SpeechModelNotice() {
  const { t } = useTranslation(NS);
  const readiness = useSeogi((s) => s.readiness);
  const config = useSeogi((s) => s.config);
  const downloading = useSeogi((s) =>
    readiness ? s.downloads[readiness.speech_model] !== undefined : false,
  );
  const later = useSeogi((s) => s.speechNoticeLater);
  const putOff = useSeogi((s) => s.putOffSpeechNotice);

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
  const bytes = readiness.speech_model_bytes;
  // Someone who chose this model lost it; anyone else is offered the
  // better default.
  const chosen = readiness.speech_model_chosen;

  return (
    <div
      role="status"
      className="flex flex-wrap items-center gap-x-3 gap-y-1 mb-2 px-3 py-2 rounded-[9px] bg-s1 text-[13.5px]"
    >
      <b>
        {t(chosen ? "speechNotice.chosenMissing" : "speechNotice.title", {
          model: shortModel(model),
        })}
      </b>
      <span className="t3 basis-full order-last">
        {t(chosen ? "speechNotice.chosenStandIn" : "speechNotice.standIn", {
          model: shortModel(readiness.speech_model_stand_in),
        })}
        {bytes > 0 && !downloading
          ? ` · ${t("settings.speechSize", {
              gb: gigabytes(bytes),
              minutes: minutesToDownload(bytes),
            })}`
          : null}
      </span>
      <span className="ml-auto flex items-center gap-3">
        {downloading ? null : (
          <button type="button" className="lnk" onClick={putOff}>
            {t("speechNotice.later")}
          </button>
        )}
        <SpeechDownload model={model} solid />
      </span>
    </div>
  );
}
