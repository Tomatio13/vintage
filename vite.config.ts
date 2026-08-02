import fs from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;
// @ts-expect-error process is a nodejs global
const forcePolling =
  process.env.CHOKIDAR_USEPOLLING === "true" ||
  // @ts-expect-error process is a nodejs global
  process.env.VITE_USE_POLLING === "1";

/** Fall back to polling when the kernel inotify table cannot accept another watch (ENOSPC). */
function shouldUsePolling(): boolean {
  if (forcePolling) return true;
  // @ts-expect-error process is a nodejs global
  if (process.platform !== "linux") return false;
  try {
    const watcher = fs.watch(
      new URL("./package.json", import.meta.url),
      () => undefined,
    );
    watcher.close();
    return false;
  } catch {
    return true;
  }
}

const usePolling = shouldUsePolling();

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],
  optimizeDeps: {
    // The syntax-highlighting worker imports highlight.js lazily, so Vite only
    // discovers it at first use and reloads the page mid-session. Pre-bundling
    // it at startup prevents that dev-only reload from clearing the app state
    // (tabs/terminals) the first time a code file is previewed.
    include: ["highlight.js"],
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
      // Avoid ENOSPC when the system inotify watch table is exhausted.
      usePolling,
      ...(usePolling ? { interval: 400 } : {}),
    },
  },
}));
