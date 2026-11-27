// The changes made both here and on the phone that said different things
// (issue #67): the later one is in the minutes, the other stays here as a
// faint line, and pressing it takes that one instead. Nothing disappears
// without a trace.
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type Conflict, type Meeting } from "@/bindings";
import { NS } from "../i18n";
import { clock } from "../lib/format";

/** What a conflict is about, in the meeting's own words. */
function about(meeting: Meeting, key: string): { kind: string; what: string } {
  const [kind, target] = [
    key.slice(0, key.indexOf(":")),
    key.slice(key.indexOf(":") + 1),
  ];
  if (kind === "done") {
    const entry = (meeting.minutes?.[0]?.sections ?? [])
      .flatMap((s) => s.entries)
      .find((e) => e.id === target);
    return { kind, what: entry?.text ?? "" };
  }
  if (kind === "line") return { kind, what: clock(Number(target)) };
  if (kind === "speaker") {
    const s = meeting.transcript?.speakers.find(
      (sp) => String(sp.id) === target,
    );
    return { kind, what: s?.name ?? target };
  }
  return { kind, what: target };
}

export function OtherEdits({
  meeting,
  onChanged,
}: {
  meeting: Meeting;
  onChanged: () => Promise<void>;
}) {
  const { t } = useTranslation(NS);
  const [conflicts, setConflicts] = useState<Conflict[]>([]);
  const [trouble, setTrouble] = useState("");

  useEffect(() => {
    let live = true;
    void commands.seogiEditConflicts(meeting.id).then((r) => {
      if (live && r.status === "ok") setConflicts(r.data ?? []);
    });
    return () => {
      live = false;
    };
  }, [meeting]);

  if (conflicts.length === 0) return null;
  return (
    <section className="a-sec" aria-labelledby="other-edits-h">
      <h2 id="other-edits-h">{t("otherEdits.title")}</h2>
      <p className="m-0 mb-1 text-[12.5px] t3">{t("otherEdits.lead")}</p>
      {conflicts.map((c) => {
        const { kind, what } = about(meeting, c.key);
        const side = t(
          c.other_side === "phone" ? "otherEdits.phone" : "otherEdits.computer",
        );
        const value =
          kind === "done"
            ? t(
                c.other === "true"
                  ? "otherEdits.ticked"
                  : "otherEdits.unticked",
              )
            : `「${c.other}」`;
        return (
          <button
            key={c.key}
            type="button"
            className="block w-full text-left border-0 bg-transparent p-0 py-1 text-[13px] t3 hover:t2"
            onClick={() =>
              void commands
                .seogiTakeOtherEdit(meeting.id, c.key)
                .then(async (result) => {
                  if (result.status === "error") {
                    setTrouble(t("failed", { reason: result.error }));
                    return;
                  }
                  setTrouble("");
                  await onChanged();
                })
            }
          >
            {t(
              `otherEdits.${kind === "done" || kind === "line" || kind === "speaker" ? kind : "line"}`,
              {
                what,
              },
            )}
            {" · "}
            {t("otherEdits.made", { side, value })}
            <span className="ml-2 underline">{t("otherEdits.take")}</span>
          </button>
        );
      })}
      {trouble ? (
        <p role="alert" className="m-0 mt-1 text-[13px] rt">
          {trouble}
        </p>
      ) : null}
    </section>
  );
}
