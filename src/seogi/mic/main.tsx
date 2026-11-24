// The floating microphone key's window.

import React from "react";
import ReactDOM from "react-dom/client";
import "../i18n";
import "../seogi.css";
import MicButton from "./MicButton";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <MicButton />
  </React.StrictMode>,
);
