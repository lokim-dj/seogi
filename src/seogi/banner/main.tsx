// The banner window: a meeting is starting, or minutes are ready.

import React from "react";
import ReactDOM from "react-dom/client";
import "../i18n";
import "../seogi.css";
import Banner from "./Banner";

// It floats over other apps; a stray right-click must not open the
// webview's own menu.
if (import.meta.env.PROD) {
  document.addEventListener("contextmenu", (event) => event.preventDefault());
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Banner />
  </React.StrictMode>,
);
