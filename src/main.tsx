// Derived from Handy (https://github.com/cjpais/Handy), Copyright (c) 2025 CJ Pais, MIT License. See THIRD_PARTY_NOTICES.md.

import React from "react";
import ReactDOM from "react-dom/client";
import { platform } from "@tauri-apps/plugin-os";
// The window is Seogi's own UI (src/seogi). The dictation settings screens
// in App.tsx are kept in the tree but not mounted.
import SeogiApp from "./seogi/App";
import { installCompatShims } from "./lib/compat";
import {
  applyTheme,
  getStoredTheme,
  syncThemeFromSettings,
} from "./lib/utils/theme";

installCompatShims();

// Set platform before render so CSS can scope per-platform (e.g. scrollbar styles)
document.documentElement.dataset.platform = platform();

// Apply the last-known theme synchronously before render to avoid a flash of
// the wrong palette, then reconcile with the persisted setting once it loads.
applyTheme(getStoredTheme());
syncThemeFromSettings();

// Initialize i18n
import "./i18n";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <SeogiApp />
  </React.StrictMode>,
);
