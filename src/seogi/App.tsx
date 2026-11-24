// Seogi's window: one screen at a time. A first run shows the choices to
// make; a recording in progress takes the window over; otherwise the home,
// a meeting's minutes, or the settings.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { platform } from "@tauri-apps/plugin-os";
import { NS } from "./i18n";
import Home, { FIND_EVENT } from "./screens/Home";
import Minutes from "./screens/Minutes";
import Onboarding from "./screens/Onboarding";
import Recording from "./screens/Recording";
import Settings from "./screens/Settings";
import { isDocument } from "./components/DocumentWords";
import { TrashedNote } from "./components/DeleteMeeting";
import { useSeogi } from "./store";
import "./seogi.css";

interface DownloadProgress {
  model_id: string;
  percentage: number;
}

/** Where a key takes the window, if anywhere. */
function keyMove(event: KeyboardEvent): string | null {
  const target = event.target as HTMLElement | null;
  const typing =
    !!target &&
    (target.isContentEditable ||
      ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName));
  // On a Mac the menu bar has these keys; answering them here too would
  // move twice.
  const mac = platform() === "macos";
  const command = !mac && event.ctrlKey;
  if (command && !event.altKey && event.key === "[") return "back";
  if (command && !event.altKey && event.key === "]") return "forward";
  if (command && event.key === ",") return "settings";
  if (event.altKey && !command && !typing) {
    if (event.key === "ArrowLeft") return "back";
    if (event.key === "ArrowRight") return "forward";
  }
  return null;
}

/** Goes where a menu item, key or button says. */
function moveTo(where: string) {
  const state = useSeogi.getState();
  if (state.recording) return;
  switch (where) {
    case "back":
      state.goBack();
      break;
    case "forward":
      state.goForward();
      break;
    case "home":
      state.go({ name: "home" });
      break;
    case "settings":
      state.go({ name: "settings" });
      break;
    case "find":
      state.go({ name: "home" });
      window.setTimeout(() => window.dispatchEvent(new Event(FIND_EVENT)), 50);
      break;
    case "dictation":
      state.go({ name: "settings" });
      window.setTimeout(
        () =>
          document
            .getElementById("dictation")
            ?.scrollIntoView({ block: "start" }),
        300,
      );
      break;
  }
}

export default function SeogiApp() {
  const { t } = useTranslation(NS);
  const loaded = useSeogi((s) => s.loaded);
  const init = useSeogi((s) => s.init);
  const config = useSeogi((s) => s.config);
  const view = useSeogi((s) => s.view);
  const isRecording = useSeogi((s) => s.recording !== null);
  const refreshRecording = useSeogi((s) => s.refreshRecording);
  const refreshSetup = useSeogi((s) => s.refreshSetup);
  const importPaths = useSeogi((s) => s.importPaths);
  const dropDocuments = useSeogi((s) => s.dropDocuments);
  const [dropping, setDropping] = useState(false);

  useEffect(() => {
    void init();
  }, [init]);

  // The clock and the level meter, while recording.
  useEffect(() => {
    if (!isRecording) return;
    const timer = window.setInterval(() => void refreshRecording(), 250);
    return () => window.clearInterval(timer);
  }, [isRecording, refreshRecording]);

  // Model downloads report their progress; the screens that care read it.
  useEffect(() => {
    const progress = listen<DownloadProgress>(
      "model-download-progress",
      (event) => {
        useSeogi.setState((state) => ({
          downloads: {
            ...state.downloads,
            [event.payload.model_id]: event.payload.percentage,
          },
        }));
      },
    );
    const complete = listen<string>("model-download-complete", (event) => {
      useSeogi.setState((state) => {
        const downloads = { ...state.downloads };
        delete downloads[event.payload];
        return { downloads };
      });
      void refreshSetup();
    });
    return () => {
      void progress.then((off) => off());
      void complete.then((off) => off());
    };
  }, [refreshSetup]);

  // The floating microphone key's menu asks for the dictation settings.
  useEffect(() => {
    const opened = listen("seogi-open-dictation", () => moveTo("dictation"));
    // The menu bar's items that move about the app (macOS).
    const menu = listen<string>("seogi-menu", (event) => moveTo(event.payload));
    return () => {
      void opened.then((off) => off());
      void menu.then((off) => off());
    };
  }, []);

  // Keys and mouse buttons that move about the app where the menu bar does
  // not: Alt+← / Alt+→ and Ctrl+, on Windows, ⌘[ / ⌘] as a fallback, and a
  // mouse's back and forward buttons. A text field keeps its own keys.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const move = keyMove(event);
      if (!move) return;
      event.preventDefault();
      moveTo(move);
    };
    const onMouse = (event: MouseEvent) => {
      if (event.button === 3) moveTo("back");
      if (event.button === 4) moveTo("forward");
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mouseup", onMouse);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mouseup", onMouse);
    };
  }, []);

  // A recording dropped on the window is brought in.
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "over") setDropping(true);
      else if (event.payload.type === "drop") {
        setDropping(false);
        // Documents dropped on an open meeting are for its words; anything
        // else is a recording to bring in.
        const paths = event.payload.paths;
        const current = useSeogi.getState().view;
        if (
          current.name === "minutes" &&
          paths.length > 0 &&
          paths.every(isDocument)
        ) {
          dropDocuments({ id: current.id, paths });
        } else void importPaths(paths);
      } else setDropping(false);
    });
    return () => {
      void unlisten.then((off) => off());
    };
  }, [importPaths, dropDocuments]);

  if (!loaded || !config) return <div className="min-h-full bg-desk" />;

  let screen;
  if (!config.set_up) screen = <Onboarding />;
  else if (isRecording) screen = <Recording />;
  else if (view.name === "minutes") screen = <Minutes id={view.id} />;
  else if (view.name === "settings") screen = <Settings />;
  else screen = <Home />;

  return (
    <div className="h-full overflow-y-auto">
      {screen}
      <TrashedNote />
      {dropping && config.set_up ? (
        <div className="fixed inset-3 z-20 flex items-center justify-center border border-dashed border-t3 rounded-[14px] bg-bg/90 text-[16px] font-medium pointer-events-none">
          {t("home.dropHere")}
        </div>
      ) : null}
    </div>
  );
}
