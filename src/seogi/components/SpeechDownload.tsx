// A speech model's download, where it is offered: the key, its progress
// with the minutes left, and why it did not work (no room on the disk, or
// another failure) with the key to try again.

import { useTranslation } from "react-i18next";
import { NS } from "../i18n";
import { gigabytesShort } from "../lib/speech";
import { useSeogi } from "../store";

export default function SpeechDownload({
  model,
  solid = false,
}: {
  model: string;
  /** The filled key, as in the home's band. */
  solid?: boolean;
}) {
  const { t } = useTranslation(NS);
  const percent = useSeogi((s) => s.downloads[model]);
  const left = useSeogi((s) => s.downloadMinutesLeft[model]);
  const failed = useSeogi((s) => s.downloadFailed.includes(model));
  const short = useSeogi((s) => s.downloadShort[model]);
  const download = useSeogi((s) => s.downloadSpeechModel);

  if (percent !== undefined) {
    const shown = Math.round(percent);
    return (
      <span className="t2">
        {left
          ? t("settings.downloadingLeft", { percent: shown, minutes: left })
          : t("settings.downloading", { percent: shown })}
      </span>
    );
  }
  return (
    <>
      {short !== undefined ? (
        <span className="rt">
          {t("settings.noSpace", { gb: gigabytesShort(short) })}
        </span>
      ) : failed ? (
        <span className="rt">{t("settings.downloadFailed")}</span>
      ) : null}
      <button
        type="button"
        className={solid ? "btn solid !h-[26px]" : "btn"}
        onClick={() => void download(model)}
      >
        {t(failed ? "settings.downloadAgain" : "settings.download")}
      </button>
    </>
  );
}
