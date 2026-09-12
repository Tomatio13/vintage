# VINTAGE repository guidance

## Scope

- This file applies to the entire repository.
- Put directory-specific guidance in the nearest nested `AGENTS.md`; more specific guidance overrides this file within that subtree.

## Architecture documentation

- Human-oriented architecture write-up for reuse in other products: `docs/architecture.md`.
- When changing a hard architecture boundary (host vs renderer, workspace path rules, ACP sanitization, lifecycle teardown, or the host façade), update `docs/architecture.md` in the same change as `AGENTS.md`.

## Repository map

- The experimental native workspace lives in `crates/vintage-{core,terminal,runtime,gpui}`; route native process and filesystem operations through `vintage-runtime`, never through GPUI render or input callbacks.
- `crates/vintage-core/src/workspace.rs` owns the pure native workspace/tab/split model; keep OS and GPUI access out of it.
- `crates/vintage-runtime/src/files.rs` owns registered, read-only native file access; `crates/vintage-gpui/src/files.rs` owns the file tree and preview tasks, and `crates/vintage-core/src/document.rs` prepares bounded document display data.
- `crates/vintage-runtime/src/native_sessions.rs` owns startup and cleanup workers; `crates/vintage-runtime/src/hook_ipc.rs` owns authenticated loopback reports; `crates/vintage-gpui/src/workspace.rs` owns workspace chrome and terminal view entities.
- ## Workflow

- Accept community bug reports, feature requests, and documentation proposals through GitHub Issues.
- Do not ask an external contributor to open a pull request until a maintainer has accepted the issue and agreed on its scope.
- For maintainer work or an invited contribution, branch from `develop` and open the pull request against `develop`.
- Do not commit directly to `main`.
- Promote releases by merging `develop` into `main`; each version update reaching `main` creates a draft GitHub Release.
- Keep each change focused and preserve unrelated working-tree changes.

## Commands

- Install dependencies: `pnpm install`
- Run the native desktop app: `cargo +1.95.0 run -p vintage-gpui --locked --`
- Build the native desktop app: `cargo +1.95.0 build -p vintage-gpui --release --locked`
- Run all native checks: `cargo +1.95.0 fmt --all --check && cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings && cargo +1.95.0 test --workspace --locked`
- Build the Debian package: `tools/package-gpui-deb.sh`

## Architecture constraints

- Do not access processes or the filesystem directly from the React renderer.
- In the native spike, own PTYs and terminal models in the runtime service; parse output and perform PTY I/O on workers, and explicitly join shutdown outside the GPUI main thread.
- On native session shutdown, cancel blocked PTY readiness waits before joining workers outside the GPUI main thread.
- Keep hidden native tabs alive; close their PTYs only when their pane, tab, workspace or application is closed, and join startup/cleanup workers outside the UI thread.
- Keep native view refresh tasks owned by the view; coalesce wakeups without carrying PTY bytes, and close update listeners when the session stops.
- The native boundary is a same-process module boundary, not process isolation; validate native service inputs and strip inherited production Hook credentials before launching a spike shell.
- Resolve native file requests through the registered workspace file service and normalized relative paths; reject canonical paths outside its root and nonregular preview targets.
- Keep native file reads and directory enumeration on workers; revoke the file service and cancel view tasks on Files close or workspace switch, and never pass filesystem paths or remote URLs to GPUI image loaders.
- Keep native preferences in `vintage-runtime/src/settings.rs`, validate reads and writes, and save atomically on workers to the Preview-specific settings directory.
- Keep the native spike independent of production application data and update channels; it must not import data or install integrations automatically.
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
- When changing native settings, test value limits, shortcut conflicts, damaged-file recovery and failed atomic saves; verify live application and restart restoration separately.
- When changing `crates/` or shared shell code, also run `pnpm check:gpui`; Windows and Linux GUI/IME checks remain distinct from compilation and unit tests.
- When changing `tools/performance/`, run its Python unit tests and verify only synthetic terminal data is captured.
- When changing production split-tree or agent-state logic, add pure TypeScript tests under `tests/` and run `pnpm test:workspace` / `pnpm test:agents`.
- When changing native file access, test workspace identity, traversal/symlink rejection, regular-file checks, byte limits and revoked access; use only synthetic file contents for GUI captures.
- When changing native workspace layout or lifecycle, add Rust model/runtime tests and exercise tab switching, recursive split collapse and terminal cleanup with synthetic PTYs.
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
