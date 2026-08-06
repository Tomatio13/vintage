import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { initializeAppearance } from "./appearance";
import { initializeFontScale } from "./fontScale";
import { initializeTerminalRuntime } from "./terminal/runtime";

initializeAppearance();
initializeFontScale();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

// The application chrome renders before terminal WASM initialization. Terminal
// surfaces await this shared promise before creating a PTY, so StrictMode
// cannot start a pane twice.
void initializeTerminalRuntime().catch(() => undefined);
