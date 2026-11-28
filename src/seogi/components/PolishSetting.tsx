// 설정 › 녹음 › 받아쓴 글 다듬기 (#108): one switch, off until the user
// turns it on. On, it says which model polishes (the one that writes the
// minutes) and whether the text leaves the computer for it, with a way to
// the minutes' model to change it.

import { useTranslation } from "react-i18next";
import type { Config } from "@/bindings";
import { NS } from "../i18n";
import { KEY_OWNERS, keyOwnerOf, modeOf } from "../lib/summary";

export default function PolishSetting({
  config,
  model,
  onSave,
  onChangeModel,
}: {
  config: Config;
  /** The minutes' model as the summary settings name it. */
  model: string;
  onSave: (config: Config) => void;
  onChangeModel: () => void;
}) {
  const { t } = useTranslation(NS);
  const mode = modeOf(config.summary);
  const who =
    config.summary.kind === "claude_cli"
      ? "Claude"
      : config.summary.kind === "codex_cli"
        ? "ChatGPT"
        : mode === "key"
          ? (KEY_OWNERS.find((item) => item.id === keyOwnerOf(config.summary))
              ?.label ?? t("onboarding.key"))
          : t("onboarding.local");
  return (
    <div className="srow">
      <div>
        <div className="l">{t("polish.title")}</div>
        <div className="h">
          {t("polish.hint")}
          {config.polish ? (
            <>
              <span className="block t2">
                {t("polish.with", { who, model })}{" "}
                <button
                  type="button"
                  className="lnk text-[13px]"
                  onClick={onChangeModel}
                >
                  {t("polish.change")}
                </button>
              </span>
              <span className="block t2">
                {t(`onboarding.content.${mode}`)}
              </span>
            </>
          ) : null}
        </div>
      </div>
      <div className="v">
        <input
          type="checkbox"
          role="switch"
          className="tg"
          checked={config.polish}
          aria-label={t("polish.title")}
          onChange={(event) =>
            onSave({ ...config, polish: event.target.checked })
          }
        />
      </div>
    </div>
  );
}
