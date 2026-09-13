# VINTAGE native architecture

VINTAGE is a single native Rust application built with GPUI. GPUI owns rendering and input; runtime services own all process and filesystem operations.

## Ownership

- `vintage-core`: pure terminal identity, composition, split, tab, and pane-kind models.
- `vintage-terminal`: terminal parsing, screen state, selection, mouse and focus protocols.
- `vintage-runtime`: shell validation, PTYs, workspace-scoped files, settings, Integration asset management, and authenticated Hook IPC.
- `vintage-gpui`: window chrome, settings, tabs, panes, terminal rendering, and file UI.

## Trust boundaries

GPUI callbacks do not spawn processes or read files. `vintage-runtime` validates shell executables and workspace roots, starts PTYs on workers, and joins their workers outside the GPUI main thread.

File access is resolved from a registered workspace root and normalized relative paths. Canonical targets outside the root, symlink escapes, nonregular previews, and over-limit content are rejected. Markdown rendering resolves image targets into normalized relative paths and loads them through the same workspace file service on workers; remote images are never fetched, and no filesystem path or URL is ever handed to a GPUI image loader.

Every application launch starts a new loopback-only Hook IPC server with a fresh 256-bit token. The token is injected only into a live PTY child environment. Reports must have the matching token, a live pane ID, the current generation, and a supported state. Tokens, terminal output, prompts, source content, and credentials are never persisted or logged.

## Lifecycle

Closing a pane stops its PTY and revokes its Hook IPC pane registration. Viewer panes opened from Files hold no PTY; each registers its own workspace file service, loads read-only previews on workers, and releases the service when the pane closes. Closing Files revokes the workspace file service. Application shutdown stops all PTYs and joins runtime workers; Hook IPC shuts down when its final runtime owner is released.

## Persistence

Settings are stored atomically under `vintage-gpui-preview/settings.json` in the OS configuration directory. They contain appearance, font, shell, scrollback, and shortcut preferences only. Terminal output, process state, Hook credentials, and workspace file contents are never persisted.
