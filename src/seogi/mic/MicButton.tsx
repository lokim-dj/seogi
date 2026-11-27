// The floating microphone key: Seogi's mark on an ink circle, above every
// other app. A press starts listening, a second press ends it and the words
// go where the cursor is. While it listens the mark's red dot blinks and a
// red ring shows; while the words are being written down a thin arc turns
// inside the ring (held still when the system asks for less motion). It can
// be dragged anywhere (the app remembers where); while it is dragged a × shows
// at the bottom of the screen and dropping it there hides it. A right click
// offers to hide it or open the settings. It never takes the keyboard.

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { platform } from "@tauri-apps/plugin-os";
import { commands, events } from "@/bindings";
import { Mark } from "../components/Mark";
import { NS } from "../i18n";
import { keysLabel, keysShort } from "../lib/format";

export type MicState = "idle" | "listening" | "working";

/** Moving the pointer this far (px) makes a press a drag. */
const DRAG_PX = 3;

export default function MicButton({
  initial = "idle",
}: {
  initial?: MicState;
}) {
  const { t } = useTranslation(NS);
  const [state, setState] = useState<MicState>(initial);
  const press = useRef<{ x: number; y: number; dragged: boolean } | null>(null);
  // The shortcut, shown when the pointer rests on the key; read again when
  // the settings change it.
  const [keys, setKeys] = useState("");

  useEffect(() => {
    const mac = platform() === "macos";
    const read = () =>
      void commands
        .seogiDictation()
        .then(({ shortcut }) => {
          const label = mac
            ? keysShort(shortcut, true)
            : keysLabel(shortcut, false);
          setKeys(label.replace(/space/i, "Space"));
        })
        .catch(() => setKeys(""));
    read();
    const off = events.dictationChanged.listen(read);
    return () => void off.then((stop) => stop());
  }, []);

  useEffect(() => {
    const offs = [
      events.recordingStateEvent.listen((event) => {
        const phase = event.payload.phase;
        if (phase === "started" || phase === "locked") setState("listening");
        else if (phase === "stopped") setState("working");
        else setState("idle");
      }),
      events.transcriptionCompleteEvent.listen(() => setState("idle")),
      listen("hide-overlay", () =>
        setState((now) => (now === "working" ? "idle" : now)),
      ),
    ];
    return () => {
      for (const off of offs) void off.then((stop) => stop());
    };
  }, []);

  const onPointerDown = (event: React.PointerEvent) => {
    if (event.button !== 0) return;
    press.current = { x: event.screenX, y: event.screenY, dragged: false };
  };

  const onPointerMove = (event: React.PointerEvent) => {
    const start = press.current;
    if (!start || start.dragged) return;
    const far =
      Math.abs(event.screenX - start.x) > DRAG_PX ||
      Math.abs(event.screenY - start.y) > DRAG_PX;
    if (far) {
      start.dragged = true;
      // The system drags the window; the app shows the × meanwhile and
      // hides the key if it is let go there.
      void commands
        .seogiMicDrag()
        .catch((error: unknown) =>
          console.warn("The microphone key could not be dragged:", error),
        );
    }
  };

  const onPointerUp = (event: React.PointerEvent) => {
    const start = press.current;
    press.current = null;
    if (event.button !== 0 || !start || start.dragged) return;
    void commands.seogiDictationToggle();
  };

  const onContextMenu = (event: React.MouseEvent) => {
    event.preventDefault();
    void commands.seogiMicMenu(
      t("dictation.mic.hide"),
      t("dictation.mic.settings"),
    );
  };

  const label =
    state === "listening"
      ? t("dictation.mic.listening")
      : keys
        ? t("dictation.mic.labelKeys", { keys })
        : t("dictation.mic.label");

  return (
    <div className="mic-stage">
      <button
        type="button"
        className={`mic-key ${state}`}
        aria-label={label}
        title={label}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onContextMenu={onContextMenu}
      >
        <Mark size={22} state={state === "listening" ? "rec" : "idle"} />
        {state === "working" ? (
          <svg className="mic-arc" viewBox="0 0 32 32" aria-hidden="true">
            <circle cx="16" cy="16" r="14" />
          </svg>
        ) : null}
      </button>
      <style>{MIC_CSS}</style>
    </div>
  );
}

// Fixed to the ink palette whatever the system's theme: the key floats over
// any app and should read the same on all of them.
const MIC_CSS = `
.mic-stage {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  user-select: none;
  -webkit-user-select: none;
}
.mic-key {
  --ink: oklch(0.19 0.007 65);
  --edge: oklch(0.34 0.008 65);
  --red: oklch(0.63 0.2 29);
  width: 34px;
  height: 34px;
  border-radius: 50%;
  border: 1px solid var(--edge);
  background: var(--ink);
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  cursor: default;
  box-shadow: 0 2px 8px oklch(0.1 0.01 60 / 0.35);
  transition: border-color 160ms ease-out, transform 120ms ease-out;
}
.mic-key:hover { border-color: oklch(0.5 0.01 70); }
.mic-key:active { transform: scale(0.94); }
.mic-key:focus { outline: none; }
.mic-key.listening {
  border: 2px solid var(--red);
}
/* A quarter of a thin circle just inside the ring, turning once a second
   while the words are being written down. */
.mic-arc {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
  animation: mic-arc-turn 1.1s linear infinite;
}
.mic-arc circle {
  fill: none;
  stroke: oklch(0.93 0.008 75 / 0.75);
  stroke-width: 1.5;
  stroke-linecap: round;
  stroke-dasharray: 22 66;
}
@keyframes mic-arc-turn {
  to { transform: rotate(360deg); }
}
@media (prefers-reduced-motion: reduce) {
  .mic-arc { animation: none; }
}
`;
