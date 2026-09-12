# VINTAGE

VINTAGE is a native GPUI workspace for terminal agents. It runs real interactive shells in tabs and split panes, provides a workspace file panel, and shows Codex, Claude Code, and OpenCode hook activity.

## Run

Rust 1.95.0 is required.

```bash
cargo +1.95.0 run -p vintage-gpui --locked --
```

Use `cargo +1.95.0 test --workspace --locked` to run the native test suite.

## Features

- Interactive PTY-backed terminal panes with tabs, splits, copy/paste, IME, scrollback, and configurable shortcuts.
- Registered workspace file browsing and safe previews.
- Settings for Graphite, Dark, Light, and System appearance; font, shell, scrollback, and shortcuts.
- Managed Codex, Claude Code, and OpenCode integrations with authenticated loopback Hook IPC.
- Linux Debian package: `tools/package-gpui-deb.sh`.

## Keyboard shortcuts

Default shortcuts can be changed in **Settings → Shortcuts**.

- `Ctrl+Shift+N`: new terminal
- `Ctrl+Shift+D`: split right
- `Ctrl+Shift+T`: split down
- `Ctrl+Shift+W`: close pane
- `Ctrl+Shift+C` / `Ctrl+Shift+V`: copy / paste

## Development

```bash
cargo +1.95.0 fmt --all --check
cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.95.0 test --workspace --locked
tools/package-gpui-deb.sh
```

See [the architecture guide](docs/architecture.md) for ownership and trust boundaries. Japanese documentation is available in [README_JP.md](README_JP.md).
