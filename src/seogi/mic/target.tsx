// The window of the × the floating microphone key is dropped on.

import React from "react";
import ReactDOM from "react-dom/client";
import DropTarget from "./DropTarget";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <DropTarget />
  </React.StrictMode>,
);
