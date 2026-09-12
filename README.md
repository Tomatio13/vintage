<p align="center">
  <img src="./assets/readme/hero.svg" width="100%" alt="VINTAGE runs coding agents in real split terminals and shows which one needs attention">
</p>

<p align="center">
  <strong>Visual Interface for Terminal Agents.</strong><br>
  An open-source multi-agent desktop workspace.
</p>

<p align="center">
  Run Codex, Claude Code, and OpenCode side by side in real terminal panes, and follow every agent's progress from one place.
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
> VINTAGE is an early release under active development. Features, compatibility, and stored preferences may change between releases.

> [!IMPORTANT]
> VINTAGE is an independent, unofficial project. It is not affiliated with or endorsed by the companies behind the supported agent CLIs.

## One workspace. Every agent in view.

VINTAGE gives every coding agent a real terminal pane and rolls its activity up into one workspace. Run Codex, Claude Code, OpenCode, or any shell command in parallel, then switch directly to the tab or pane that needs attention.

<p align="center">
  <img src="./docs/assets/vintage-main.png" width="100%" alt="VINTAGE desktop interface with a workspace sidebar, split terminal panes, and file preview">
</p>

## What VINTAGE does

- **Runs real terminals.** Each pane owns an interactive pseudoterminal (PTY) shell with input, resize, copy/paste, input method editor support, scrollback, and explicit teardown.
- **Organizes parallel work.** Renameable tabs and recursively split panes let several agents work inside the same registered project folder.
- **Surfaces attention.** Managed Codex, Claude Code, and OpenCode integrations report activity through authenticated local Hook IPC. Activity is shown in the pane header, tab, and sidebar.
- **Keeps files close.** Open the Files panel from the workspace header, resize it by dragging its edge, browse the registered workspace tree, and preview supported files safely.
- **Keeps controls configurable.** Settings include Graphite, Dark, Light, and System appearance, terminal font and scrollback, default shell, and keyboard shortcuts.
- **Stays native.** v0.5 replaces the previous Tauri application with the GPUI desktop application. The GPUI edition is now the only VINTAGE desktop build.

## Current support

- **Platforms:** Linux x86-64 and Windows x86-64 packages.
- **Terminals:** Interactive shell panes, tabs, recursive right/down splits, divider resizing, tab renaming, and close actions.
- **Workspace files:** Registered-workspace browsing and bounded, read-only previews. The Files panel never accepts arbitrary filesystem paths or remote URLs.
- **Agents:** Codex, Claude Code, and OpenCode managed integrations, plus any command entered in a terminal shell.
- **Activity:** Authenticated Hook IPC reports agent state per pane. Install the managed asset from **Settings → Integrations**, then start a new agent session.
- **Settings:** Graphite, Dark, Light, and System color modes; font family and size; default shell; scrollback; and configurable shortcuts.

## Install VINTAGE

VINTAGE runs agent command-line interfaces directly on your computer. Install the CLIs you intend to use, then choose the VINTAGE package for your operating system. Each CLI retains ownership of its sign-in and credentials.

### 1. Install agent CLIs

Install any supported agent with its official installer and confirm that it is on `PATH`:

```bash
codex --version     # OpenAI Codex
claude --version    # Claude Code
opencode --version  # OpenCode
```

### 2. Download VINTAGE

Download only from the official [GitHub Releases page](https://github.com/Tomatio13/vintage/releases/latest). Under **Assets**, choose one of these files:

- **Windows x86-64:** `VINTAGE_*_x64-setup.exe`
- **Debian or Ubuntu x86-64:** `VINTAGE_*_amd64.deb`
- **Other Linux x86-64 distributions:** `VINTAGE_*_amd64.AppImage`

#### Windows

Run `VINTAGE_*_x64-setup.exe` and follow the installer. Confirm that the file was downloaded from `github.com/Tomatio13/vintage` before continuing.

#### Debian or Ubuntu

```bash
sudo apt install ./VINTAGE_*_amd64.deb
```

#### Other Linux distributions

```bash
chmod +x VINTAGE_*_amd64.AppImage
./VINTAGE_*_amd64.AppImage
```

## Getting started

1. Open a project folder from the workspace sidebar. That registered folder becomes the trust root for its terminals and Files panel.
1. Start a terminal. New terminals use the selected default shell.
1. Run an agent directly in the shell, for example `codex`, `claude`, or `opencode`.
1. Create parallel work with the workspace-header split controls or shortcuts.
1. Open **Files** from the workspace header to browse and preview the project.
1. Install optional agent hooks from **Settings → Integrations** when you want activity reports in the workspace chrome.

Default shortcuts can be changed in **Settings → Shortcuts**:

- `Ctrl+Shift+N`: new terminal
- `Ctrl+Shift+D`: split right
- `Ctrl+Shift+T`: split down
- `Ctrl+Shift+W`: close pane
- `Ctrl+Shift+C` / `Ctrl+Shift+V`: copy / paste

When VINTAGE exits, it stops terminal sessions and its local Hook IPC service. Do not rely on VINTAGE to silently restart agent processes after a quit.

## How it works

```text
GPUI desktop view
      │ validated workspace and terminal requests
      ▼
VINTAGE runtime service
      │ PTY, file service, and Hook IPC
      ▼
Shell process running the agent CLI
```

The GPUI view does not directly launch processes or read arbitrary files. The runtime service owns shell detection, executable validation, terminal lifecycle, registered-workspace file access, previews, settings persistence, and local Hook IPC.

Each terminal pane runs an interactive PTY shell. The runtime stops the session when its pane, tab, workspace, or application closes. File operations resolve from the registered workspace and reject traversal, symlink escapes, nonregular preview targets, and oversized display data.

For the full ownership and trust-boundary model, see [docs/architecture.md](docs/architecture.md).

## Setting up agent hooks

VINTAGE can receive activity reports from Codex, Claude Code, and OpenCode through managed hook or plugin assets.

1. Open **Settings → Integrations**.
1. Select **Install** for the agent you use.
1. Start a new interactive agent session in a VINTAGE terminal pane.

VINTAGE adds, updates, and removes only the configuration entries and files it owns. An existing conflicting asset is reported instead of overwritten. The local report channel is authenticated and loopback-only; its token is not persisted or logged.

## Privacy and security

- VINTAGE does not read or store agent credentials or browser-authentication tokens.
- Terminal output, prompts, source code, and Hook IPC tokens are not persisted or logged by VINTAGE.
- Terminals have the same local access as the shell that runs them. Review commands before executing them.
- File access is restricted to registered workspaces, and previews are bounded before display.
- Closing a pane, tab, workspace, or the application stops its terminal sessions. Removing a workspace never deletes its directory or files.
- Agent CLIs and their services may retain their own data independently. Consult each agent's documentation for its policy.

## Development

Requirements:

- Rust 1.95.0
- A supported Linux or Windows desktop environment
- Any agent CLI you want to exercise manually

```bash
git clone https://github.com/Tomatio13/vintage.git
cd vintage
cargo +1.95.0 run -p vintage-gpui --locked --
```

Run the native checks:

```bash
cargo +1.95.0 fmt --all --check
cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.95.0 test --workspace --locked
```

Build local packages:

```bash
tools/package-gpui-deb.sh
tools/package-gpui-appimage.sh
```

### Tech stack

- Rust 2021 and GPUI
- `alacritty_terminal` with a runtime-owned pseudoterminal
- Native workspace, terminal, settings, file, and Hook IPC services
- GitHub Actions for Linux `.deb` / AppImage and Windows setup packages

## Contributing

Community contributions are accepted through [GitHub Issues](https://github.com/Tomatio13/vintage/issues/new/choose). Read [CONTRIBUTING.md](CONTRIBUTING.md) before submitting a report or proposal.

- Search existing issues before opening a new one.
- Use the appropriate form for a bug, feature request, or documentation improvement.
- Keep each issue focused on one topic and remove sensitive information from reports and attachments.
- Do not open a pull request unless a maintainer invites you to implement an accepted issue.

## Releasing

<details>
<summary>Maintainer release process</summary>

1. Update the workspace version in `Cargo.toml` and create the matching Git tag.
1. Push the release commit and tag to GitHub.
1. Run **Publish release assets** from the Actions page and enter the tag, such as `v0.5.1`.
1. The workflow creates or updates that Release and attaches the Linux AppImage, Linux Debian package, and Windows setup executable.
1. Test the artifacts on their target operating systems before promoting the Release.

The workflow publishes these assets:

- `VINTAGE_<version>_amd64.AppImage`
- `VINTAGE_<version>_amd64.deb`
- `VINTAGE_<version>_x64-setup.exe`

</details>

## License

VINTAGE is available under the [MIT License](LICENSE).
