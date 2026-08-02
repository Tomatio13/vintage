import React from "react";
import ReactDOM from "react-dom/client";
import { init } from "ghostty-web";
import App from "./App";
import { initializeAppearance } from "./appearance";
import { initializeFontScale } from "./fontScale";

initializeAppearance();
initializeFontScale();

void (async () => {
  // Load ghostty-web's WASM once before rendering so Terminal components can
  // be created synchronously (StrictMode double-invokes effects; deferring the
  // async init into each terminal made two panes start the same PTY).
  try {
    await init();
  } catch {
    // A failed WASM load surfaces as a terminal status error per pane.
  }
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
})();
