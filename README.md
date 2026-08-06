<p align="center">
  <img src="./assets/readme/hero.svg" width="100%" alt="VINTAGE runs coding agents in real split terminals and shows which one needs attention">
</p>

<p align="center">
  <strong>Visual Interface for Terminal Agents.</strong><br>
  An open-source multi-agent desktop workspace.
</p>

<p align="center">
  Run Grok, Codex, Claude Code, and OpenCode side by side in real terminal panes, and follow every agent's progress from one place.
</p>

<p align="center">
  <a href="./README_JP.md">日本語</a> · <strong>English</strong>
</p>

<p align="center">
  <a href="https://github.com/Tomatio13/vintage/releases/latest"><strong>Download VINTAGE</strong></a> ·
  <a href="#getting-started">Getting started</a> ·
  <a href="#what-vintage-does">Features</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

> [!WARNING]
> VINTAGE is an early release under active development. Features, compatibility, and stored navigation metadata may change between releases.

> [!IMPORTANT]
> VINTAGE is an independent, unofficial project. It is not affiliated with or endorsed by the companies behind the supported agent CLIs.

## One workspace. Every agent in view.

VINTAGE gives every coding agent a real terminal pane and rolls its activity up into one sidebar. Run Grok, Codex, Claude Code, OpenCode, or any custom command in parallel, then jump directly to the workspace that is blocked, working, or done.

<p align="center">
  <img src="./docs/assets/vintage-main.png" width="100%" alt="VINTAGE desktop interface with a workspace sidebar, split terminal panes, and file preview">
</p>

## What VINTAGE does

- **Runs real terminals.** Each pane owns an interactive shell process with configurable scrollback, input, resize, restart, and explicit teardown.
- **Organizes parallel work.** Tabs and recursively split panes let several agents work inside the same registered project folder.
- **Surfaces attention.** Agent activity rolls up from pane to tab to workspace as `blocked`, `working`, `done`, or `idle`. Screen-manifest detection works today, and Hook/Plugin reports (OpenCode plugin lifecycle, Codex/Claude session identity) take priority over screen detection once installed from Settings → Integrations.
- **Session resume is under development.** The host-side resume commands and authenticated reporting pipeline exist, but native session resume is not yet a supported end-to-end feature.
- **Keeps files close.** Browse a live workspace tree, edit and save text files, and preview images, PDF documents, and fonts without leaving the app.
- **Restores placement, not processes.** Workspaces, tabs, splits, and pane definitions survive a restart; commands are never relaunched silently.
- **Updates in place.** Signed update bundles are delivered through GitHub Releases and installed by the desktop app.

## Current support

| Area | Current support |
| --- | --- |
| Workspaces | Registered project folders are the trust source for every terminal and file operation; unregistering stops processes and watchers but never deletes files |
| Terminals | Interactive shell panes with configurable scrollback, tabs, recursive horizontal/vertical splits, divider resize, restart, and close |
| Shells | PowerShell 5.1 / 7, Git Bash, Ubuntu default shell, Bash, Zsh, fast-startup variants for Git Bash/Bash/Zsh, and validated custom executables |
| Agents | Grok, Codex, Claude Code, and OpenCode presets plus arbitrary custom programs; each runs inside a shell and returns to it when it exits |
| Activity | Screen-manifest detection (Herdr-ported) rolls sidebar badges up to the workspace. Hook/Plugin reporting is installed from Settings → Integrations and takes priority over screen detection once reporting |
| Restart | A stopped pane can restart into a fresh shell. Native session resume is under development |
| Workspace files | Live file tree with hidden-file controls, text editing and saving, system file-manager actions, and media previews |
| Layout | Placement persists to `workspace-layouts.json`; damaged files stop autosave and offer a back-up-and-reset flow |
| Updates | Signed in-app updates backed by GitHub Releases |

VINTAGE currently ships for Windows x86-64 and Linux x86-64. Supported shells include PowerShell 5.1/7, Git Bash, Bash, Zsh, the Ubuntu default shell, fast-startup variants where available, and validated custom executables.

## Install VINTAGE

VINTAGE runs each agent CLI directly on your computer. Install the CLIs you want to use (Grok, Codex, Claude Code, OpenCode), then choose the VINTAGE package for your operating system. VINTAGE detects the CLIs on your `PATH`; launching an undetected CLI still works if it is added to `PATH` after your profile loads.

### 1. Install agent CLIs

Install any of the supported agents with their official installers and confirm they are on `PATH`:

```bash
grok --version      # Grok Build
codex --version     # OpenAI Codex
claude --version    # Claude Code
opencode --version  # OpenCode
```

VINTAGE does not bundle or sign in to the agents on your behalf; each CLI owns its own authentication.

### 2. Download VINTAGE

Download VINTAGE only from the official [GitHub Releases page](https://github.com/Tomatio13/vintage/releases/latest). Expand **Assets** and select the package that matches your computer:

| Operating system | Package to download | Architecture |
| --- | --- | --- |
| Windows | `VINTAGE_*_x64-setup.exe` | x86-64 |
| Debian or Ubuntu | `VINTAGE_*_amd64.deb` | x86-64 |
| Other Linux distributions | `VINTAGE_*_amd64.AppImage` | x86-64 |

#### Windows

Run `VINTAGE_*_x64-setup.exe` and follow the installer. The Windows installer is signed for VINTAGE's in-app updater, but it is not yet Authenticode code signed, so Windows SmartScreen may show an unrecognized-publisher warning. Confirm that the file came from `github.com/Tomatio13/vintage` before continuing.

#### Debian or Ubuntu

```bash
sudo apt install ./VINTAGE_*_amd64.deb
```

#### Other Linux distributions

Make the AppImage executable, then run it:

```bash
chmod +x VINTAGE_*_amd64.AppImage
./VINTAGE_*_amd64.AppImage
```

Files ending in `.sig`, `.app.tar.gz`, and `latest.json` are used by the signed automatic updater. You do not need to download them for a manual installation.

## Getting started

Start VINTAGE from your Applications folder, Start menu, or application launcher.

1. **Open a workspace** with the + button in the sidebar, or pick an existing folder. The workspace becomes the trust root for every terminal and file operation.
1. **Start a terminal** in a pane. A new tab starts with your default shell; each pane can be split horizontally or vertically.
1. **Launch an agent** from a pane's controls, or just type its command into the shell directly (`codex`, `claude`, `opencode`, `grok`).
1. **Watch the sidebar** — badges roll each pane up to its tab and workspace, so a blocked agent is visible from the tree.
1. **Edit text files and preview files** with the **Files** toggle on the right, and open folders in your system file manager.

Useful keyboard controls:

- <kbd>Arrow</kbd> keys on a divider resize it; <kbd>Home</kbd>/<kbd>End</kbd> jump to the extremes; <kbd>Shift</kbd> makes the step finer.
- The workspace sidebar lists workspaces → tabs → panes, with attention badges on each row.
- Closing a pane, tab, or workspace stops its PTY first, then removes the placement.

When you quit VINTAGE, every PTY, child process, file watcher, and hook IPC connection is stopped. Only the placement is saved; processes are never silently relaunched.

### Speed up a slow shell prompt

Choose **Settings → Application → Default shell**, then create a new terminal. When the corresponding shell is detected, VINTAGE provides these optional fast-startup choices:

- **Git Bash (fast startup)** and **Bash (fast startup)** run with `--noprofile --norc` and skip shell profile and rc files.
- **Zsh (fast startup)** runs with `zsh -f` and skips most zsh startup files.

Fast startup is useful on lower-spec machines or profiles with many prompt plugins. It deliberately does not load aliases, themes, language-version managers, or other setup from those skipped files. Your existing shell configuration is never changed; select the normal shell again whenever that configuration is needed.

## How it works

```text
React renderer
      │ typed Tauri commands and events
      ▼
Tauri Rust host
      │ PTY + hook IPC environment
      ▼
Shell process running the agent CLI
```

The React renderer does not start processes or access the filesystem directly. The Rust host owns shell detection, executable validation, PTY creation and teardown, workspace registration and watching, file previews, and the in-progress Hook/Plugin integration infrastructure, including the local IPC server.

Each pane is a real PTY running a detected shell (PowerShell, Git Bash, Bash, Zsh, or a validated custom executable). Agent presets launch inside that shell and return to it when they exit, so the pane stays interactive. Per-pane generations guard stale host events.

Activity is designed as two separate layers that combine per pane:

- The PTY state (starting / running / stopped / exited / error) tracks the process.
- Agent activity (unknown / idle / working / blocked / done) comes from screen-manifest matching of the live bottom buffer (ported from Herdr) and, once installed from Settings → Integrations, from Hook/Plugin reports. OpenCode's plugin reports lifecycle state and takes priority over screen detection; Codex and Claude hooks report the native session id, which VINTAGE shows in the sidebar pane rows.

Placement persists to `workspace-layouts.json`; runtime state — PTY ids, scrollback, hook tokens, prompts — never does. Quitting VINTAGE stops every PTY, descendant process, file watcher, and hook IPC connection.

For a full layer map, trust boundaries, and patterns you can reuse in other desktop agent clients, see [docs/architecture.md](docs/architecture.md).

## Setting up agent hooks (Integrations)

VINTAGE reads agent activity from two sources: screen-manifest detection (always on) and, optionally, **hook/plugin reports** that the agent CLIs send on their own. Reports take priority over screen detection and also carry the agent's native session id, which VINTAGE shows in the sidebar pane rows.

To enable reports, install the managed hook/plugin for an agent from **Settings → Integrations**. Each agent gets an Install / Uninstall button and a status row.

| Agent | Asset installed | Config entry written |
| --- | --- | --- |
| Codex | `~/.codex/vintage-agent-state.sh` (or `.ps1`) | `~/.codex/hooks.json` `hooks.SessionStart` + `~/.codex/config.toml` `[features] hooks = true` |
| Claude Code | `~/.claude/hooks/vintage-agent-state.sh` (or `.ps1`) | `~/.claude/settings.json` `hooks.SessionStart` (matcher `*`) |
| OpenCode | `~/.config/opencode/plugins/vintage-agent-state.js` | none — plugins auto-load |

VINTAGE only adds, updates, or removes the entries it owns (each managed command ends in `# vintage:codex` / `# vintage:claude`). Your existing hooks, plugins, settings, and credentials are left untouched. A same-name file that is not managed by VINTAGE is reported as a **Conflict** and is never overwritten.

After installing, start a **new interactive session** of the agent inside a pane (the hook runs on session start). A native session id then appears next to the pane in the sidebar. To stop reporting, use **Uninstall**, which removes the managed entry and script but keeps your configuration.

> **Note:** Codex's `codex exec` (non-interactive) mode does not fire `SessionStart` hooks. Use an interactive session to see the session id. OpenCode's plugin works with `opencode run` as well.

## Privacy and security

- VINTAGE does not read or store agent credentials or browser-auth tokens. Each agent CLI owns its own authentication.
- VINTAGE stores registered workspace paths and placement in the operating system's application-data directory.
- VINTAGE does not persist terminal output, prompts, or source-code contents, and it does not write prompts, responses, credentials, or raw process output to application logs.
- Files opened in the right panel are read on demand. Text editing and saving is capped at 512 KiB; font previews at 8 MiB; supported image or PDF previews at 20 MiB.
- Terminal panes run your local system shell with the workspace as their current directory. Commands entered there have the same local access as that shell.
- The hook IPC token is generated in the renderer with Web Crypto, handed to the host once, injected only into PTY child environments, and never logged or persisted.
- Local processes and filesystem access are initiated through the Tauri Rust host, not the renderer.
- Unregistering a workspace stops its processes and watchers but never deletes the directory or its files.
- The agent CLIs and their services may retain their own session data independently of VINTAGE; consult the official documentation for their behavior and policies.

## Development

Requirements:

- Node.js 24 or later
- pnpm 11
- Rust 1.88 or later
- Any of the supported agent CLIs you intend to run (Grok, Codex, Claude Code, OpenCode)

Clone the repository, install dependencies, and run the desktop application:

```bash
git clone https://github.com/Tomatio13/vintage.git
cd vintage
pnpm install
pnpm tauri dev
```

To preview the renderer without starting local processes:

```bash
pnpm dev
```

### Tech stack

- Tauri 2 and Rust
- React 19, TypeScript, and Vite
- ghostty-web (Ghostty's WASM terminal emulator) over a Rust-hosted pseudoterminal
- pnpm
- A typed host facade over Tauri commands and events (no direct process/FS access from the renderer)

### Verification

```bash
pnpm check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

`pnpm check` runs the frontend typecheck and build, TypeScript timing tests, all Rust unit tests, and `cargo check`.

## Contributing

Community contributions are accepted through [GitHub Issues](https://github.com/Tomatio13/vintage/issues/new/choose). Read [CONTRIBUTING.md](CONTRIBUTING.md) before submitting a report or proposal.

- Search existing issues before opening a new one.
- Use the appropriate form for a bug, feature request, or documentation improvement.
- Keep each issue focused on one topic and remove sensitive information from all reports and attachments.
- Do not open a pull request unless a maintainer invites you to implement an accepted issue.

## Releasing

<details>
<summary>Maintainer release process</summary>

VINTAGE checks `https://github.com/Tomatio13/vintage/releases/latest/download/latest.json` for signed desktop updates. The public updater key is committed in `src-tauri/tauri.conf.json`; keep its private key outside the repository and back it up securely.

Configure these GitHub Actions secrets before the first release:

- `TAURI_SIGNING_PRIVATE_KEY` with the contents of `~/.tauri/vintage.key`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` when the updater key is password-protected

For each release:

1. Update the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
1. Open a pull request from `develop` to `main` and merge it after its required checks pass, or run the **Release** workflow manually from `main`.
1. Wait for every matrix job to finish and inspect the draft GitHub Release.
1. Test each installer on its target operating system before publishing the draft.

The [release workflow](.github/workflows/release.yml) runs when a version change in `src-tauri/tauri.conf.json` reaches `main`, and can also be run manually with `main` selected. It builds Windows NSIS and Linux AppImage/DEB artifacts, signs updater bundles, generates `latest.json`, and creates a draft GitHub Release. Publish the draft only after testing its installers.

Before publishing, confirm that the draft contains:

- The Windows NSIS `.exe` installer and its `.sig` file.
- Linux `.AppImage` and `.deb` packages and both `.sig` files.
- A `latest.json` whose version and platform entries match the uploaded updater bundles.
- Release notes that clearly describe user-visible changes and any known limitations.

After publishing, verify the public [latest release](https://github.com/Tomatio13/vintage/releases/latest) and the [updater manifest](https://github.com/Tomatio13/vintage/releases/latest/download/latest.json). Do not replace assets on a published release; ship a new patch release if an installer or updater manifest must be corrected.

An installation that predates updater support cannot discover the updater-enabled release. Existing users must install that first release manually once; later releases can update inside VINTAGE.

</details>

## License

VINTAGE is available under the [MIT License](LICENSE).
