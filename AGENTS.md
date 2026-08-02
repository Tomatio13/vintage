# VINTAGE repository guidance

## Scope

- This file applies to the entire repository.
- Put directory-specific guidance in the nearest nested `AGENTS.md`; more specific guidance overrides this file within that subtree.

## Architecture documentation

- Human-oriented architecture write-up for reuse in other products: `docs/architecture.md`.
- When changing a hard architecture boundary (host vs renderer, workspace path rules, ACP sanitization, lifecycle teardown, or the host façade), update `docs/architecture.md` in the same change as `AGENTS.md`.

## Repository map

- `src/App.tsx` is a thin shell: appearance, font scale, updates, and the settings view. It owns no conversation state.
- `src/workspace/` owns the terminal workspace: pure state model (`types.ts`, `paneLayout.ts`, `agentState.ts`, `paneRuntime.ts`), persistence mirrors (`persistence.ts`), screen-manifest detection (`screenDetection.ts`, `manifests/*.json`), the layout hook, and the UI (`WorkspaceApp.tsx`, `WorkspaceSidebar.tsx`, `WorkspaceTabs.tsx`, `SplitPaneView.tsx`, `PaneTerminal.tsx`, `WorkspaceFilesPanel.tsx`). Keep pure modules free of React and Tauri so the Node test runner covers them.
- `src/terminal/TerminalSurface.tsx` owns the xterm.js lifecycle and live bottom-buffer snapshots.
- `src/FileExplorer.tsx` owns the workspace file tree and live refresh; `src/files/FilePreview.tsx` owns preview rendering.
- `src/host/index.ts` owns typed renderer access to Tauri commands and host events; keep raw command and event names there. `src/host/types.ts` holds the wire DTOs.
- `src/settings/` owns the Application and Appearance settings UI.
- `src-tauri/src/lib.rs` owns Tauri command registration, updates, and application lifecycle.
- `src-tauri/src/shells.rs` owns shell detection, executable validation, and command construction (POSIX quoting, PowerShell EncodedCommand).
- `src-tauri/src/workspaces.rs` owns the workspace registry and `workspace-layouts.json` persistence with validation.
- `src-tauri/src/terminal.rs` owns PTY creation, I/O, resize, teardown, agent presets, and screen-state reporting.
- `src-tauri/src/file_manager.rs` owns workspace-scoped listing, preview, folder opening, and watching.
- `src-tauri/src/hook_ipc.rs` owns the local hook/plugin IPC server and token validation.
- `src-tauri/src/integrations.rs` owns hook/plugin asset install, update, and uninstall.
- Keep process, filesystem, and agent CLI access inside the Tauri host.
- Expose host functionality to the renderer through typed Tauri commands and events.
- Route renderer command calls and host event subscriptions through `src/host/index.ts` instead of scattering raw Tauri names across components.
## Workflow

- Accept community bug reports, feature requests, and documentation proposals through GitHub Issues.
- Do not ask an external contributor to open a pull request until a maintainer has accepted the issue and agreed on its scope.
- For maintainer work or an invited contribution, branch from `develop` and open the pull request against `develop`.
- Do not commit directly to `main`.
- Promote releases by merging `develop` into `main`; each version update reaching `main` creates a draft GitHub Release.
- Keep each change focused and preserve unrelated working-tree changes.

## Commands

- Install dependencies: `pnpm install`
- Run the web UI: `pnpm dev`
- Run the desktop app: `pnpm tauri dev`
- Run the frontend typecheck and build: `pnpm build`
- Run workspace pure-model tests: `pnpm test:workspace`
- Run agent state and screen-detection tests: `pnpm test:agents`
- Run appearance and font-scale tests: `pnpm test:ui`
- Run all Rust unit tests: `pnpm test:rust`
- Run Rust compilation checks: `pnpm check:rust`
- Run frontend and Rust checks: `pnpm check`
- Check Rust formatting: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`

## Architecture constraints

- Do not access processes or the filesystem directly from the React renderer.
- Run agent CLIs and shells only through the Tauri Rust host PTY commands; the renderer never spawns processes.
- Resolve workspace file operations in the Rust host from the registered workspace id only; accept normalized relative paths, and reject canonical targets outside that workspace root.
- Keep file preview size limits, filesystem watching, and system file-manager launching in `src-tauri/src/file_manager.rs`.
- Validate shell executables (regular file, executable bit on Unix, PATHEXT extension on Windows) before trusting renderer-supplied custom paths.
- Validate terminal identifiers, dimensions, input sizes, generations, and hook IPC tokens before touching a terminal session or accepting a report.
- Stop PTYs when a pane, tab, or workspace closes; stop workspace watchers when the Files panel closes or a workspace is unregistered; stop all host runtimes including hook IPC when the application exits.
- Keep Rust command payloads, emitted event names, and their TypeScript counterparts synchronized, including serde casing and optional fields.
- Do not persist prompts, terminal output, source code, credentials, or hook IPC tokens, and never log them.
- Write all user-facing UI text and host-provided error or status messages in English.

## Verification

- After changing application code, run `pnpm check` and the Rust formatting check.
- When changing split-tree or agent-state logic, add pure TypeScript tests under `tests/` and run `pnpm test:workspace` / `pnpm test:agents`.
- When porting or editing screen manifests, keep priority, region, conditions, and visible flags intact and cover the rules in `tests/screenDetection.test.ts`.
- When changing shell detection or command construction, add Rust tests covering Windows and Unix branches (runnable on any OS via injected inputs).
- When changing workspace persistence, add Rust and/or TypeScript tests covering validation limits, migration, atomic writes, and damaged-file recovery.
- When changing terminal host behavior, add Rust tests for identifier, size, input, generation, or lifecycle validation as applicable.
- When changing hook IPC, add Rust tests for token handling and report validation.
- For documentation-only changes, verify referenced commands and paths; application builds are not required.
- Report which checks were run and identify any checks that could not be run.

## Review guidelines

- Flag direct process or filesystem access from `src/`.
- Flag agent or shell launching that bypasses the typed Tauri PTY boundary.
- Flag workspace file commands that trust renderer-supplied absolute paths without resolving through the registered workspace id, or permit traversal or symlink escape outside the resolved workspace root.
- Flag shell executable validation gaps (missing executable-bit or PATHEXT checks).
- Flag PTYs that survive pane, tab, or application teardown, and flag filesystem watchers that survive Files-panel or application teardown.
- Flag stale-generation or unauthenticated hook reports that are not dropped.
- Flag mismatches between Rust command or event payloads and their renderer-side TypeScript types.
- Flag logs that may contain prompts, terminal output, source code, credentials, or hook IPC tokens.

## Maintaining this file

- Update this file in the same change when an architecture boundary, canonical command, or required verification workflow changes.
- Add guidance when the same repository-specific mistake or review feedback occurs repeatedly.
- Write one actionable instruction per bullet, using explicit conditions such as "When changing X, run Y" where applicable.
- Put guidance in the closest directory where it applies instead of expanding the root file with local details.
- Remove or revise instructions as soon as they become inaccurate.
- Do not add temporary task context, completed-work history, or general programming advice.
