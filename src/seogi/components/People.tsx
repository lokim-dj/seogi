// Who was in a meeting, kept in its people (the same as "이번 회의 정보"),
// and the names a speaker can be given: this meeting's people, or a name
// typed. Nothing is suggested from other meetings or made up: every name
// is one the user typed for this meeting, and stays on this computer.

import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { Meeting } from "@/bindings";
import { NS } from "../i18n";
import { isUnnamed } from "./common";
import { Marked } from "./Found";
import { CloseIcon } from "./icons";
import { setMeetingInfo } from "./MeetingContext";

// Earlier versions kept names given lately to suggest them again. They are
// not suggested any more; what was kept is let go.
try {
  window.localStorage.removeItem("seogi.recentNames");
} catch {
  // Nothing was kept, or storage is closed.
}

/** A name as it is kept: composed one way, spaces tidied. */
export function tidyName(name: string): string {
  return name.normalize("NFC").split(/\s+/).filter(Boolean).join(" ");
}

/** Each name once (whatever its spacing, case or encoding), leaving out some. */
export function unique(names: string[], leaveOut: string[] = []): string[] {
  const out = new Set(leaveOut.map((name) => tidyName(name).toLowerCase()));
  const kept: string[] = [];
  for (const raw of names) {
    const name = tidyName(raw);
    const key = name.toLowerCase();
    if (name && !isUnnamed(name) && !out.has(key)) {
      out.add(key);
      kept.push(name);
    }
  }
  return kept;
}

/** A small name key: pressing it chooses the name. */
function NameKey({
  name,
  onPress,
  onRemove,
}: {
  name: string;
  onPress?: () => void;
  onRemove?: () => void;
}) {
  const { t } = useTranslation(NS);
  return (
    <span className="chip !pr-1 !gap-1 !text-t1 !text-[13.5px]">
      {onPress ? (
        <button
          type="button"
          className="p-0 border-0 bg-transparent"
          onClick={onPress}
        >
          {name}
        </button>
      ) : (
        <span>{name}</span>
      )}
      {onRemove ? (
        <button
          type="button"
          aria-label={t("people.remove", { name })}
          className="grid place-items-center w-4 h-4 p-0 border-0 rounded-full bg-transparent t3"
          onClick={onRemove}
        >
          <CloseIcon className="w-2.5 h-2.5" />
        </button>
      ) : (
        <span className="w-0.5" />
      )}
    </span>
  );
}

/** A name not on any list, typed once. */
function NewName({ onAdd }: { onAdd: (name: string) => void }) {
  const { t } = useTranslation(NS);
  const [name, setName] = useState("");
  const add = () => {
    const wanted = name.trim();
    if (!wanted) return;
    onAdd(wanted);
    setName("");
  };
  return (
    <span className="inline-flex items-center gap-1.5">
      <input
        type="text"
        value={name}
        onChange={(event) => setName(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") add();
        }}
        placeholder={t("people.newName")}
        aria-label={t("people.newName")}
        className="ul !w-[120px] !min-h-[24px] !py-0.5 text-[13.5px]"
      />
      {name.trim() ? (
        <button type="button" className="lnk" onClick={add}>
          {t("people.add")}
        </button>
      ) : null}
    </span>
  );
}

/**
 * "참석 …" under a meeting's title. Pressed, it opens to add names one at a
 * time (typed, Enter adds) and to take them out (×).
 */
export function PeopleLine({
  meeting,
  onChanged,
  open: startOpen = false,
  className = "",
  mark = "",
  labelled = true,
}: {
  meeting: Meeting;
  onChanged: () => Promise<void> | void;
  open?: boolean;
  className?: string;
  /** False where the row already has "참석자" written above it. */
  labelled?: boolean;
  /** Words a search went to, marked in the names. */
  mark?: string;
}) {
  const { t } = useTranslation(NS);
  const [open, setOpen] = useState(startOpen);
  // Names saved twice before are shown once; saving tidies them for good.
  const people = unique(meeting.people ?? []);

  const save = async (next: string[]) => {
    await setMeetingInfo(meeting.id, { people: unique(next) });
    await onChanged();
  };

  if (!open) {
    return (
      <button
        type="button"
        className={`flex items-baseline gap-3 p-0 border-0 bg-transparent text-left text-[13.5px] ${className}`}
        onClick={() => setOpen(true)}
      >
        <span className="t3">{t("people.label")}</span>
        <span className={people.length > 0 ? "t2" : "t3"}>
          {people.length > 0 ? (
            <Marked text={people.join(", ")} words={mark} />
          ) : (
            t("people.none")
          )}
        </span>
        <span className="lnk">{t("people.edit")}</span>
      </button>
    );
  }
  return (
    <div className={`grid gap-2 text-[13.5px] ${className}`}>
      <div className="flex flex-wrap items-center gap-1.5">
        {labelled ? (
          <span className="t3 mr-1.5">{t("people.label")}</span>
        ) : null}
        {people.map((name) => (
          <NameKey
            key={name}
            name={name}
            onRemove={() => void save(people.filter((other) => other !== name))}
          />
        ))}
        <NewName onAdd={(name) => void save([...people, name])} />
      </div>
      <p className="m-0 text-[13px] t3">{t("people.typeHint")}</p>
      {startOpen ? null : (
        <button
          type="button"
          className="lnk justify-self-start"
          onClick={() => setOpen(false)}
        >
          {t("people.done")}
        </button>
      )}
    </div>
  );
}

/**
 * The names to give a speaker: this meeting's people as keys to press, and
 * a name typed. Pressing a name chooses it at once.
 */
export function NamePicker({
  meeting,
  current,
  onPick,
  more = [],
  children,
}: {
  meeting: Meeting;
  /** The speaker's present name, not offered again. */
  current: string;
  onPick: (name: string) => void;
  /** Names typed a moment ago, offered as keys after the people. */
  more?: string[];
  /** More keys after the names, such as "모르겠어요". */
  children?: ReactNode;
}) {
  const people = unique([...(meeting.people ?? []), ...more], [current]);
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {people.map((name) => (
        <NameKey key={name} name={name} onPress={() => onPick(name)} />
      ))}
      <NewName onAdd={onPick} />
      {children}
    </div>
  );
}
