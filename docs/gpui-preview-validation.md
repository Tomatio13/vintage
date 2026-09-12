# GPUI Preview: workspace implementation and validation

A runnable **workspace Preview** exists in `crates/`: folder workspaces, tabs,
recursive equal splits and independent terminal sessions. The GPUI application
is the intended production replacement; Tauri remains a legacy release until
the documented feature and desktop-validation gaps are closed. Desktop/IME and
performance checks below gate release qualification and production replacement.
Shared production-service extraction remains deferred.

## Build and run

- Native candidate: Rust **1.95.0**, official GPUI and `gpui_platform` at the same
  revision `7c07887d9555953bca7fe78417602114092ef8c8` (Zed v0.229.0),
  `alacritty_terminal` **0.25.1**, `portable-pty` **0.9.0**.
- These versions are fixed for reproducible spike validation, not qualified for
  release on all target desktops. The native root `Cargo.lock` is independent of
  `src-tauri/Cargo.lock`; production stays on Rust 1.88.0.
- Ubuntu build dependencies: `pkg-config`, `libfontconfig1-dev`,
  `libfreetype6-dev`, `libxkbcommon-dev`, `libxkbcommon-x11-dev`,
  `libwayland-dev`, `libvulkan-dev` and a C/C++ build toolchain.
- A missing `-lxkbcommon-x11` link means the development package is missing, even
  if `libxkbcommon-x11.so.0` is installed. For local verification on this host,
  the existing library was linked through an ignored `.data/native-libs`
  directory using `LIBRARY_PATH`; no system package was installed.
- Windows requires the Rust MSVC build prerequisites and Windows SDK. The added
  CI job targets a Windows runner; a Linux cross-check alone is not a Windows
  runtime qualification.

```sh
rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy
pnpm check:gpui
pnpm run dev:gpui --list-shells
pnpm run dev:gpui --shell unix-bash-fast --cwd /path/to/project
pnpm build:gpui
pnpm package:gpui:deb
```

The release executable is `target/release/vintage-gpui` on Linux and
`target\release\vintage-gpui.exe` on Windows. For Windows use a detected ID such
as `windows-default`, `windows-powershell` or `windows-git-bash`. `--shell` also
accepts a validated absolute executable path. `--cwd` defaults to the current
directory and is canonicalized and validated on the startup worker.

`--exit-after 1..60` requests normal application shutdown after the given number
of seconds for bounded startup/shutdown smoke tests. It does not measure input
latency or establish visual correctness.

## Debian candidate package

`pnpm package:gpui:deb` creates
`target/release/bundle/deb/vintage-gpui_0.1.0_amd64.deb`. It installs separately
from the legacy Tauri package, with the desktop ID `dev.tiebi.vintage.gpui` and
the VINTAGE SVG icon. This allows both applications to be tested without package
or launcher conflicts. It is a local candidate package, not a published release.

The script targets `amd64` and requires the Linux build dependencies listed
above. Its package metadata declares the runtime libraries used by GPUI.

On 2026-09-12, the local `vintage-gpui_0.1.0_amd64.deb` candidate was
installed on Linux and its desktop-bar icon was manually confirmed. This verifies
package installation and desktop icon association on that machine only.

## Implemented boundaries

- `vintage-core`: pure workspace/tab/split state with focus, collapse-on-close,
  16-workspace/64-total-pane/depth-8 limits; terminal size and session identity validation, special-key
  encoding, bounded bracketed paste, UTF-16 IME composition and selection ranges.
- `vintage-terminal`: ordered byte parsing, ANSI colors, wide/combining cells,
  alternate screen, 1,000 history lines, selection, cursor/query responses and
  a live bottom-screen snapshot independent of scroll position.
- `vintage-runtime`: validated shell startup, PTY I/O workers, a bounded input
  queue, resize/selection/scroll commands, generation checks and explicit
  shutdown. The spike compiles the existing shell source in place; production
  services have **not** yet been extracted into the shared runtime.
- `vintage-gpui`: workspace sidebar, tab bar, recursive terminal panes, folder
  picker, warm dark theme and production ANSI
  palette, default 12 px font, focus, IME preedit/commit, selection, clipboard,
  scrollback, cell-based mouse reports, focus notifications and font size controls.
- PTYs and terminal models belong to the service owner, outside the view.
  Parsing and native PTY operations run on workers. Output never crosses a
  JSON event boundary. A single-consumer notification coalesces state changes;
  UI invalidation reads the latest model revision. The view owns and cancels its
  refresh task; service shutdown wakes a waiting receiver.
- This is a **same-process module boundary**, not Tauri's renderer/host process
  separation. Production Hook environment variables are removed from spike
  children. The native Settings screen can install, update, and remove its
  managed Codex, Claude Code, and OpenCode hook/plugin assets without
  overwriting user-owned configuration. The spike runs an authenticated,
  loopback-only Hook IPC server for its own live PTY panes and shows reported
  working, blocked, and idle states in the native workspace. Production-data
  import and the updater remain unavailable.

## Controls

- Type or use the platform IME to enter text; preedit stays local until commit.
- Drag with the left mouse button to select; Ctrl+Shift+C copies the selection;
  Ctrl+Shift+V pastes, respecting bracketed paste mode.
- Scroll with the wheel or Shift+PageUp/PageDown. Hold Shift to bypass an
  application's mouse reporting and select/scroll locally.
- Ctrl+plus, Ctrl+minus and Ctrl+0 adjust or reset the terminal font size.
- Applications can request click (1000), drag (1002), or all-motion (1003)
  reporting. Left/middle/right buttons, vertical wheel, Ctrl/Alt modifiers,
  legacy coordinates, UTF-8 coordinates (1005), and SGR coordinates (1006) are
  implemented. Motion is reported only when the cell changes. Shift starts
  local selection instead; a previously reported press still receives its
  release if Shift is pressed mid-drag.
- Applications requesting focus reports (1004) receive activation/deactivation
  notifications. Enabling the mode synchronizes the current focus once; repeated
  callbacks do not duplicate it. Window activation and terminal keyboard focus
  must both be present. Leaving the terminal cancels the local drag gesture.
- Resize the window to resize the PTY. Close the window to terminate its session.
- Sidebar **+** (Ctrl+Shift+O) opens a workspace folder using the OS picker.
  Tab **+** (Ctrl+Shift+N) adds a shell using the startup `--shell` choice.
- **Split right / Split down** (Ctrl+Shift+D/T) splits the active pane equally;
  splits can nest. Pane-header **│ / ─** buttons split that pane.
- Pane **×** (Ctrl+Shift+W) closes its terminal and collapses the split; the last
  pane closes the tab. Tab and workspace **×** close all contained terminals.
- Click sidebar/tab entries to switch; Ctrl+Shift+Tab cycles tabs. Hidden tabs
  retain their processes and output. Empty workspaces can create a new tab.
- Layouts are in-memory only. Split ratios are fixed at 50/50; drag resizing,
  saved layouts, settings and agent status/Hooks are not yet implemented.

## Files and window controls

- **Files** or Ctrl+Shift+F toggles a read-only file tree and preview for the active
  workspace. Click folder arrows to expand/collapse; click a file to preview it.
- UTF-8 text has line numbers and horizontal scrolling. Markdown supports basic
  headings, paragraphs, unordered lists, quotes, rules and fenced code blocks;
  **Source / Preview** switches views. Inline markup, tables and embedded images
  are not fully rendered; links and HTML are not executed or fetched.
- **Copy** copies the loaded text (up to the runtime preview limit). **Refresh**
  reloads the tree and selected file. There is no filesystem watcher or editor.
- PNG/JPEG previews receive only validated bytes from the runtime. Image decode
  errors and unsupported binary types display a message, not a blank preview.
- Limits: 2,000 entries per directory, 10,000 cached tree entries, 128 expanded
  folders; text 512 KiB, 8,000 displayed lines and 4,096 characters per line;
  Markdown 1,000 blocks; images 8 MiB, 16 megapixels and 8,192 pixels per side.
  Truncated previews are marked. Closing Files or switching workspaces revokes
  its file service, cancels view tasks and releases the displayed image asset.
- Drag the top bar (including the area near the right-hand Preview label) to
  request OS window movement. On Linux a left press arms the drag and the first
  motion starts it; release/outside press clears it. Windows uses GPUI's native
  drag hitbox. Double-click toggles maximization on Linux; separate buttons
  minimize, maximize/restore and close the window.

## Reproducible synthetic checks

`synthetic_terminal.py` is an executable POSIX fixture. It never executes typed
input or writes terminal contents to disk. It renders Japanese, wide and
combining characters, emoji, styles and terminal dimensions; `b` emits 10,000
synthetic lines, `a` switches to an alternate screen, `r` requests the cursor
position and `q` exits.

```sh
pnpm run dev:gpui --shell "$PWD/tools/performance/synthetic_terminal.py"
python3 -m unittest discover -s tools/performance -p 'test_*.py'
python3 tools/performance/smoke_x11.py --output .data/gpui-smoke-new
python3 tools/performance/smoke_x11.py --ime --output .data/gpui-ime-new
```

- The X11 helper requires `xwininfo`, `xprop`, ImageMagick's `import`, and
  `libX11.so.6`. It refuses an existing report directory.
- It verifies the Preview window's PID before sending events. It captures only
  the synthetic window, sends a normal close request and waits for exit code 0.
- Default mode disables XIM **for the launched process only**, to test direct
  input. `--ime` preserves the current XIM configuration. It does not change the
  desktop's IME settings or write to the clipboard.
- Screenshots require visual inspection. Completing the helper is not an
  automatic assertion that text rendered correctly.

## Linux resource collector

`sample_linux.py` uses Python's standard library and procfs. It attaches to an
existing application PID and samples that process and its current descendants.
It does not read terminal contents, command lines or environment variables, or
modify application data.

```sh
python3 tools/performance/sample_linux.py \
  --pid 12345 --implementation tauri --revision COMMIT_HASH \
  --panes 4 --scenario idle --display wayland \
  --duration 30 --interval 0.25 --output /tmp/vintage-tauri-4-idle.json
```

- Replace PID and revision with actual values. Pane count, display and workload
  are operator-supplied labels, not detected facts. Match them to the actual
  Preview pane count before taking a measurement.
- Repeat production measurements for 1/4/8 panes, idle and synthetic continuous
  output, on matching release builds and the same machine, terminal size, font,
  scrollback, shell and display backend. Record these controls with each report.
- RSS is summed resident memory; shared pages are counted more than once. Also
  obtain proportional set size before making a total-memory claim.
- CPU is relative to one core: 100% is one fully occupied core. Short-lived or
  reparented processes can be missed. Newly observed processes contribute CPU
  only after their first sample.
- The collector refuses existing output and aborts if the root process exits or
  its PID is reused. A launcher that exits is not a suitable root.
- It does not measure launch-to-window, visible prompt or input-to-paint latency.
  Those need presentation-aware instrumentation or external observation;
  PTY-byte arrival must not be substituted for visible paint.
- No current-release baseline or performance comparison has been captured.
  Windows resource collection remains unimplemented.

## Evidence and open gate

Recorded on 2026-09-08:

- Linux native compilation and executable linking succeeded with Rust 1.95.0.
- Pure tests cover fragmented UTF-8, colors, wrapping, cursor replies, alternate
  screen, wide-character selection, resize and live-bottom behavior while the
  viewport is scrolled. IME tests cover UTF-16 boundaries and oversized input.
- Linux real-PTY tests cover input, resize, stale generation, input limits,
  final output before natural exit, a shell ignoring HUP, and immediate/repeated
  startup/shutdown. Shared shell tests exercise Windows and Unix construction
  branches. Production Hook environment isolation is tested.
- On this **Wayland desktop's XWayland server**, the X11 smoke screenshots were
  inspected for direct input, resize, Japanese/combining/emoji rendering,
  10,000-line output completion and shell-exited status. Normal window close
  exited with code 0.
- XWayland with IBus (`XMODIFIERS=@im=ibus`) displayed `にほんご` as underlined
  preedit and then as terminal text after Return. This is not a native Wayland
  IME test or a full candidate-selection compatibility qualification.
- Linux-to-Windows GNU cross-compilation succeeded. It does not verify MSVC
  linking, Windows execution or ConPTY/IME behavior.
- The existing `pnpm check` passed. A separate Windows/Linux native CI workflow
  is added, but has not been run remotely.

### Final verification and baseline attempt

Final local verification repeated `pnpm check`, `pnpm check:gpui`, production
Rust formatting, the five Python collector tests and `git diff --check`;
all passed. Native checks used the local `LIBRARY_PATH` described above.

The isolated production baseline attempt did not produce usable resource or
latency measurements. Its recorded application PID was no longer running when
work resumed. The ignored `.data/production-baseline/` files are setup artifacts,
not performance evidence.

An existing wire-format mismatch was found while preparing a saved test layout:
`src-tauri/src/workspaces.rs` uses `PaneLayout::Leaf { pane_id }`, while
`src/workspace/types.ts` expects `paneId`. The enum's `rename_all` changes variant
names, not fields inside variants. This production persistence issue is outside
the native spike change and remains unresolved. Before resuming a baseline,
either fix it in a separate change with a cross-boundary round-trip test, or
create the synthetic pane layout through the running UI and verify its count.

### Mouse interaction follow-up (2026-09-08)

- The user reported briefly launching and operating the Preview. This is not a
  full platform/IME qualification; no additional desktop environment is inferred.
- Added a pure mouse encoder and wired it through parser snapshots to GPUI
  button, motion and wheel events. Unit tests cover button identity, releases,
  modifiers, tracking modes, coordinate bounds and terminal mode resets.
- `pnpm check`, `pnpm check:gpui`, production Rust formatting and a native debug
  executable build passed after this change.
- A dedicated synthetic-window input attempt did not deliver mouse events:
  the helper used core X11 events, whereas the pinned GPUI backend handles
  XInput button/motion events. This attempt is not GUI compatibility evidence.
  The application was terminated by the harness after its failed assertion.
- Manual follow-up: in an application that enables mouse handling, verify all
  three buttons and dragging; hold Ctrl/Alt and then Shift for local selection;
  move/release outside the terminal, and verify later movement does not extend
  an old selection. Repeat on Windows and native X11/Wayland.
- Protocol reference: [Xterm mouse tracking](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h2-Mouse-Tracking).

### Focus notification follow-up (2026-09-08)

- Added application-requested focus notifications through the existing bounded
  PTY input queue, with window-activation and terminal-focus subscriptions owned
  by the view. Inactive windows no longer paint the active terminal cursor.
- Four new tests cover enable/disable/reset, duplicate suppression,
  resynchronization, and retrying the latest state after a rejected send.
- `pnpm check`, `pnpm check:gpui`, production Rust formatting and native debug
  linking passed. A two-second synthetic XWayland startup/shutdown smoke exited
  with code 0. Desktop focus-switching behavior still needs manual validation
  on each target backend; unit tests do not establish OS event delivery.
- Manual check: enable focus reporting in a terminal application, switch away
  and back, and verify one loss/gain report. Disable the mode and repeat to verify
  silence. Also switch away during local text selection and verify that moving
  the pointer after returning does not extend the old selection.

### Visible text layout reuse (2026-09-08)

- The view now retains shaped text for the currently displayed nonblank cells.
  Text/style changes rebuild the affected entry; font size or display-scale
  changes invalidate the cache. Selection, background and position changes do
  not rebuild text. Entries removed from the visible frame are dropped, and
  oversized vector capacity is reclaimed; no terminal history is accumulated.
- GPUI already caches underlying glyph layout. This change avoids repeated
  VINTAGE-side string, text-run and decorated-layout construction as well; it
  does not claim that GPUI previously reshaped every glyph from scratch.
- Four new tests verify reuse, all text-style invalidations, font/scale changes,
  and release of removed layouts. A synthetic 80-by-24 unchanged screen over ten
  frames invokes the layout factory 1,920 times instead of 19,200. This measures
  call counts, not latency or CPU improvement. Extra retained layouts need to
  be included in the future memory comparison.
- `pnpm check`, `pnpm check:gpui`, production Rust formatting and native debug
  linking passed. The XWayland synthetic smoke completed with exit code 0;
  resized input and 10,000-line output screenshots were visually inspected for
  Japanese, combining characters, emoji, colors and styles. These results do
  not qualify Windows, native Wayland or IME candidate selection.

### Event-driven view refresh (2026-09-08)

- Replaced the view's permanent 16 ms polling loop with a single-consumer async
  update notification implemented using the standard library. The notification
  stores one pending flag and one waker; it never carries or drops PTY bytes.
- Output, resize, selection, write completion, errors and shell exit notify the
  view. Registering a waiter and checking pending changes share a lock to avoid
  lost wakeups. Shutdown closes the listener, and dropping the view cancels its
  task and releases the registered waker.
- Startup still polls its background JoinHandle. During active updates, a 16 ms
  delay coalesces bursts. Snapshot-lock contention and rejected focus writes
  trigger a retry; an idle, healthy, started view has no periodic refresh timer.
- Five new tests cover idle wait, burst coalescing, notification-before-wait,
  concurrent registration, close/drop behavior, and a real PTY resize followed
  by shutdown. Native checks, production `pnpm check`, formatting and debug
  linking passed. The synthetic XWayland smoke exited normally; its screenshot
  showed the 10,000-line completion marker and shell-exited status.
- PTY reader and child-supervisor polling are unchanged, so this is not a claim
  that all native background wakeups are eliminated. CPU, memory and latency
  improvements still need controlled measurements.

### Unix reader readiness and exited-session inspection (2026-09-09)

- The previous reader-polling note describes the state before this follow-up.
  Unix readers now wait in OS `poll` without a periodic timeout. Service shutdown
  closes a dedicated socket peer before joining workers, waking blocked reads.
  Owned descriptors are close-on-exec; hangup permits final output to drain.
- The handoff records passing native and production checks, debug linking,
  Windows GNU cross-checking, and four readiness tests for idle cancellation,
  final data, cancellation priority and descriptor ownership. Its XWayland smoke
  exited with code 0; the final screenshot was inspected for the 10,000-line
  completion marker and shell-exited status. These are prior-run evidence.
- Shell exit now rejects input and resize while allowing local scroll, selection
  and clearing selection. A real-PTY regression test exercises those operations
  after natural exit, followed by explicit worker shutdown.
- Current-run verification passed `pnpm check`, `pnpm check:gpui`, production
  Rust formatting and `git diff --check`. The new real-PTY regression passed;
  no GUI smoke or Windows runtime check was repeated in this run.
- Exited-session snapshots suppress application mouse/focus reporting and the
  active cursor, including modes parsed from late final output. Drag and wheel
  therefore remain local even if the application left mouse tracking enabled.
  The view stops requesting PTY resize after exit when the window/font changes;
  the final grid keeps its existing dimensions. The regression test checks
  late modes, actual scroll offset and selected text rather than revision counts.
- Writer and child-supervisor periodic waits remain. No CPU, memory or latency
  improvement is inferred, and no additional desktop backend is qualified.

### Workspace UI implementation (2026-09-09)

- Replaced the single-terminal frame with a workspace sidebar, switchable tab
  bar and recursive equal splits. Pane/tab/workspace close removes only owned
  views and native sessions; closing the final tab leaves an empty workspace.
- Moved startup and cleanup ownership into `vintage-runtime::native_sessions`.
  Shutdown joins all visible/hidden/startup/closing sessions outside the UI thread,
  rejects starts after shutdown, and retains cleanup errors for the final check.
  Only the focused terminal registers the platform text input handler.
- Pure Rust model tests cover nested split collapse, focus fallback, tab and
  workspace switching, close ownership, duplicate folders and capacity/depth
  limits. Runtime tests cover close during startup, bulk shutdown and retained
  errors. `pnpm check:gpui` runs these tests alongside the terminal tests.
- `.data/gpui-workspace-smoke-20260909/` records the synthetic XWayland keyboard
  smoke: split into three panes, create a second tab, restore the original tab,
  close panes, resize, type and drain 10,000 lines, then close the application
  with exit code 0. Child-process counts matched each create/close operation.
  The three-pane screenshot was visually reviewed.
- `.data/gpui-workspace-dialog-check/` records a separate synthetic-window check:
  a folder returned by the OS picker created a second workspace; sidebar clicks
  switched workspaces; mouse clicks on split buttons created three panes in the
  first workspace. Closing that workspace reduced live child processes from
  four to one. Closing the remaining tab reduced them to zero; creating another
  tab restored one. Application shutdown exited with code 0. The switch and
  three-pane screenshots were visually reviewed. Captured terminal content is
  generated only by `synthetic_terminal.py`, never by a user's shell.
- Final verification passed `pnpm check`, `pnpm check:gpui`, production Rust
  formatting, the five Python performance-tool tests, Windows GNU workspace
  cross-checking (`--locked`), native debug linking and `git diff --check`.
  The final debug binary repeated the keyboard smoke with exit code 0 in
  `.data/gpui-workspace-final-20260909/`; its three-pane screenshot was reviewed.
- These checks cover this host's XWayland path. They do not qualify Windows,
  native Wayland, IME candidate selection, or production performance. Split
  ratios are equal and fixed, and layouts are not saved between launches.

Reproduce the keyboard workspace smoke with:

```sh
python3 tools/performance/smoke_x11.py --workspace --output .data/gpui-workspace-new
```

### Files and title bar follow-up (2026-09-09)

- Added the runtime file boundary, bounded document preparation and Files UI.
  New tests cover workspace identity/revocation, path traversal, symlink escape,
  FIFO rejection, UTF-8 truncation, binary/empty/removed files, image dimension
  limits, Markdown blocks and display truncation.
- Synthetic XWayland screenshots in `.data/gpui-files-check/` were inspected for
  Markdown (including Japanese), line-numbered source and PNG preview.
  The maximize button changed the window's maximized state.
- The automated drag attempt reached GPUI's OS movement API but window coordinates
  did not change under this XTest/XWayland harness. This is not a passed physical
  drag test. The handler follows the pinned GPUI platform title bar's press/move
  sequence; physical pointer movement and Windows/native Wayland controls still
  require confirmation. Temporary diagnostic output was removed from the code.
- Final verification (2026-09-10): `pnpm check`, `pnpm check:gpui`, production
  Rust formatting, Windows GNU workspace cross-checking (`--locked`) and native
  debug linking passed. The final synthetic-window check selected a file by
  clicking beyond its filename, reviewed Japanese Markdown and monospace code
  in `markdown-final.png`, and shut down with exit code 0. Screenshots contain
  only synthetic fixture data. Physical title-bar dragging remains unverified.

### Settings and window resizing follow-up (2026-09-10)

- The user confirmed file viewing and physical title-bar movement on their desktop.
  The earlier automated drag failure does not negate that separate manual result.
- Settings now cover appearance, UI text scale, terminal font/size, scrollback,
  default shell, six customizable navigation bindings, and safe installation or
  removal of managed Codex, Claude Code, and OpenCode hook/plugin assets.
  Preview updates remain unavailable, so settings do not yet provide complete
  Tauri feature parity.
- Preferences live in the OS config directory under
  `vintage-gpui-preview/settings.json`; `--settings PATH` supplies an isolated
  test location. Writes validate values and atomically replace the settings file
  on a worker. Corrupt files remain intact until an explicit save.
- Synthetic XWayland checks in `.data/gpui-files-check/` verified Light appearance,
  live terminal size change from 12 to 14 px, persistence of 2,500-line scrollback,
  Ctrl+F6 rebinding and navigation, and restoration after restart. No user shell
  output was captured; both terminals used the synthetic fixture.
- Four-edge/four-corner resize handles request the native resize API. Synthetic
  pointer dragging did not change window geometry, so physical resizing remains
  unverified. An X11 size request changed the owned window from 1280×800 to
  1024×640; workspace/settings layout and terminal dimensions followed. This
  verifies resize handling, not a physical drag gesture. Both test launches
  shut down with exit code 0.
- The final 200% UI-scale screenshot (`settings-200-final.png`) confirms readable
  navigation, stacked controls and a visible Save button. Terminal font size
  remains independent of UI scale.
- Final verification recorded on 2026-09-11: `pnpm check`, `pnpm check:gpui`
  (78 native tests), production Rust formatting, Windows GNU workspace
  cross-checking (`--locked`), native debug linking and `git diff --check` passed.
  Windows and native Wayland GUI behavior remain separate desktop checks.

### Pane close control regression (2026-09-11)

- Restored the pane-header close control lost during the settings follow-up.
  **× Close** remains visible by allowing the shell label to shrink; mouse-down
  does not change selection before closing an inactive pane.
- The synthetic XWayland click check closed one inactive pane in a four-pane tab.
  The remaining three panes, active pane and another tab were preserved. The
  owned synthetic child-process set decreased from five to four, with exactly
  the clicked pane's process removed. Screenshots and process counts are in
  `.data/gpui-files-check/pane-close-{before,after}.png` and
  `pane-close-processes.json`. The test application exited with code 0.

Still required before release qualification/production replacement:

- Native Windows and Linux X11/Wayland daily-use checks, including Japanese
  conversion/candidate selection, clipboard and all four agent interfaces.
- Windows PowerShell/Git Bash PTY execution, native rendering, and lifecycle
  tests. An automated Windows PTY test is supplied for CI but was not run here.
- Background/disowned-job and hostile-child cleanup across both OSes; current
  lifecycle evidence covers the owned shell and its foreground process group,
  not arbitrary detached descendants.
- Complete terminal interaction coverage: desktop validation of the implemented
  mouse modes; additional protocols such as pixel coordinates; application
  keypad; desktop validation of focus reporting; cursor shapes/blinking; and selection/clipboard races
  under sustained output remain unqualified.
- Drawing remains per visible cell, with shaped-text reuse for unchanged cells.
  Batched drawing and PTY-worker idle wakeup improvements remain; the new view
  notification path still needs profiling before performance qualification.
- Release baseline and comparison: at least 20% less memory with four panes;
  startup and input-latency p95 no worse than 110% of production.
- Remaining product implementation: saved layouts, adjustable split ratios,
  agent activity/manifests/Hooks, software updates, file editing and public package
  release. Common production-service extraction and explicit atomic data
  import remain separate work; production is not migrated automatically.

No GPUI release has been published. The Debian candidate installs beside the
legacy application; production data, install locations and `latest.json` remain
unchanged until the final migration.
