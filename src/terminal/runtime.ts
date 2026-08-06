/** Shared, single-flight initialization for the terminal WebAssembly runtime. */

import { init } from "ghostty-web";

let initialization: Promise<void> | null = null;

export function initializeTerminalRuntime(): Promise<void> {
  if (initialization === null) initialization = init();
  return initialization;
}
