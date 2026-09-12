# VINTAGE

VINTAGE は、ターミナルエージェント向けのネイティブ GPUI ワークスペースです。実際の対話シェルをタブと分割ペインで実行し、ワークスペースのファイル表示と Codex／Claude Code／OpenCode の Hook 状態表示を提供します。

## 起動

Rust 1.95.0 が必要です。

```bash
cargo +1.95.0 run -p vintage-gpui --locked --
```

ネイティブテストは次で実行します。

```bash
cargo +1.95.0 test --workspace --locked
```

## 主な機能

- PTY を使う対話ターミナル、タブ、分割、IME、コピー／ペースト、スクロールバック、変更可能なショートカット。
- 登録済みワークスペースの安全なファイル一覧とプレビュー。
- Graphite、Dark、Light、System の配色、フォント、シェル、スクロールバック、ショートカット設定。
- Codex、Claude Code、OpenCode の安全な Integration と認証付き loopback Hook IPC。
- Linux 向け Debian パッケージ: `tools/package-gpui-deb.sh`。

## ショートカット

既定ショートカットは **Settings → Shortcuts** で変更できます。

- `Ctrl+Shift+N`: 新しいターミナル
- `Ctrl+Shift+D`: 右へ分割
- `Ctrl+Shift+T`: 下へ分割
- `Ctrl+Shift+W`: ペインを閉じる
- `Ctrl+Shift+C` / `Ctrl+Shift+V`: コピー／ペースト

## 開発

```bash
cargo +1.95.0 fmt --all --check
cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.95.0 test --workspace --locked
tools/package-gpui-deb.sh
```

所有権と信頼境界は [アーキテクチャガイド](docs/architecture.md) を参照してください。英語版は [README.md](README.md) です。
