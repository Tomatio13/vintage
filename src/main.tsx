import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { initializeAppearance } from "./appearance";
import { initializeFontScale } from "./fontScale";

initializeAppearance();
initializeFontScale();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
