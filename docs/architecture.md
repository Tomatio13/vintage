# VINTAGE アーキテクチャ

この文書は、複数のコーディングエージェントをデスクトップで並行実行するターミナルワークスペース **VINTAGE** のソフトウェアアーキテクチャを説明します。エージェントを実ターミナルで起動し、ワークスペースを管理し、特権操作を UI プロセスの外に置く、といった他のデスクトップアプリでも同じパターンを再利用できるよう書いています。

他製品へ持ち運べる中核は、**レイヤ分割・信頼境界・型付きホスト境界・純粋な状態モデル・ライフサイクル規約・状態の二層化**です。

______________________________________________________________________

## 1. プロダクトの形

| 関心 | VINTAGE の選択 |
| --- | --- |
| アプリシェル | [Tauri 2](https://tauri.app/)（Rust ホスト + webview） |
| UI | React 19 + TypeScript + Vite |
| エージェント実行環境 | 各 CLI（Grok / Codex / Claude Code / OpenCode）を検出済みシェル内で起動 |
| 端末 | `portable-pty` による PTY。xterm.js が描画 |
| 永続化 | ワークスペース登録（`working-directories.json`）と配置（`workspace-layouts.json`）のみ |
| 状態判定 | 画面マニフェスト照合（Herdr移植）と、Settings → Integrations で導入する Hook／Plugin 報告。後者が画面判定より優先 |

**設計意図:** UI は **投影（projection）と操作面** に徹する。プロセス起動、ファイルシステム、資格情報、エージェント CLI は **Rust ホストだけ** が持つ。

______________________________________________________________________

## 2. 全体図

```text
┌──────────────────────────────────────────────────────────────────────┐
│                      Renderer（React）                                │
│  App.tsx（薄い殻） · workspace/* · TerminalSurface · FileExplorer ·    │
│  Settings                                                             │
│         ▲                                                │            │
│         │  型付き API のみ                               │            │
│         │  src/host/index.ts                             │            │
└─────────┼────────────────────────────────────────────────┼────────────┘
          │ invoke(command) / listen(event)                │
┌─────────▼────────────────────────────────────────────────▼────────────┐
│                        Host（Rust / Tauri）                           │
│  lib.rs          コマンド登録、更新、ライフサイクル                   │
│  shells.rs       シェル検出、実行ファイル検証、コマンド生成           │
│  workspaces.rs   ワークスペース登録、レイアウト永続化、検証           │
│  terminal.rs     PTY の start/write/resize/stop、エージェント起動     │
│  file_manager.rs ワークスペース限定 FS、プレビュー、監視              │
│  hook_ipc.rs     ローカル IPC、トークン検証                           │
│  integrations.rs Hook／Plugin の管理                                 │
└─────────┬────────────────────────────────────────────────┬────────────┘
          │ PTY / env                                    │ FS / 監視
          ▼                                                ▼
   シェルプロセス（エージェント）                  ワークスペース + ファイル
```

**データ面:** PTY 出力 → ホスト → Tauri イベント → xterm 描画。下端スナップショット → 純粋な画面照合 → 活動状態。

**制御面:** ユーザー操作 → `host.*` → Tauri コマンド → ホスト検証 → PTY / FS。

______________________________________________________________________

## 3. アーキテクチャ原則

### 3.1 特権の隔離

| ホストで許可 | レンダラで禁止 |
| --- | --- |
| シェル／エージェントプロセスの起動 | プロセスの直接 spawn |
| ワークスペース内のファイル読み書き | 絶対パスでの FS アクセス |
| Hook IPC トークンの保持・子環境への注入 | トークンの保持・ログ・永続化 |
| Integration 設定ファイルの変更 | 管理外ファイルの上書き |
| PTY の入出力 | 端末プロセスの生制御 |

**理由:** webview の侵害や UI のバグが、そのまま全 FS や資格情報へのアクセスになってはいけない。

### 3.2 単一の型付きホスト・ファサード

- **1 モジュール** がコマンド名、イベント名、ペイロード整形を所有する（`src/host/index.ts`）。
- ホスト DTO の型は `src/host/types.ts`、ドメイン型は `src/workspace/types.ts` に置く。
- Rust の `#[serde(rename_all = "camelCase")]` と TypeScript のフィールド名は常に同期する。

### 3.3 信頼境界での検証

レンダラからの入力はすべて検証する: ワークスペース ID はレジストリ経由で解決、相対パスは正規化＋ルート配下を要求、シェル実行ファイルは通常ファイル＋実行ビット／PATHEXT、PTY 寸法と入力サイズ、Hook 報告はトークン・ペイン ID・世代番号。

### 3.4 純粋な状態モデル

分割ツリー操作、状態集約、永続化検証、画面照合は **純粋関数** にする。

- 入力: 直前の状態 + 操作/イベント +（任意）時計／ID ファクトリ。
- 出力: 新しい状態。
- 投影内に I/O・Tauri・React hooks を置かない。

`src/workspace/`（`paneLayout.ts`／`agentState.ts`／`paneRuntime.ts`／`persistence.ts`／`screenDetection.ts`）が該当し、Node テストランナーでカバーする。

### 3.5 ケイパビリティとしてのワークスペースルート

ファイル操作は、レンダラが渡した絶対パスをワークスペースルートとして信用しない。ルートは **登録済みワークスペース ID** だけから解決する。正規化した相対パスのみ受け付け、canonical 化してルート配下に留まることを要求する。

### 3.6 明示的なライフサイクルと解体

| リソース | 開始 | 停止 |
| --- | --- | --- |
| PTY | ペイン起動 | ペイン／タブ／ワークスペース閉鎖、アプリ終了 |
| ファイル監視 | Files パネルがワークスペースを対象にしたとき | パネル閉鎖、ワークスペース登録解除、アプリ終了 |
| Hook IPC | 初回 `hook_ipc_initialize` | アプリ終了 |

ワークスペースの明示的な削除では、関連 PTY とファイル監視を停止し、レジストリと保存レイアウトから対象を取り除く。ワークスペースディレクトリとその内容は削除しない。

アプリ終了時、ホストは全ランタイムを停止する（`shutdown_app`）。Drop ガードは最終防衛であり、唯一の計画にしてはいけない。

### 3.7 状態の二層化

PTY 状態（starting / running / stopped / exited / error）とエージェント活動状態（unknown / idle / working / blocked / done）は別物。前者はプロセス、後者はエージェントの作業状況。`done` は導出値で、非表示ペインの working→idle（または正常終了）で生成し、閲覧で確認する。

### 3.8 ログの衛生

プロンプト、Terminal 出力、ソースコード、資格情報、Hook IPC トークンをログに残さない。ユーザー向けエラーは英語・短文・秘密情報なし。

______________________________________________________________________

## 4. レイヤ対応表

### 4.1 レンダラの所有範囲

| 領域 | 主な場所 | 責務 |
| --- | --- | --- |
| アプリ殻 | `src/App.tsx` | appearance、fontScale、更新、Settings 切替 |
| ホスト・ファサード | `src/host/index.ts` | 型付きコマンドと購読 |
| ホスト DTO | `src/host/types.ts` | ワークスペース、端末、Integration、更新 |
| ドメイン型 | `src/workspace/types.ts` | 分割ツリー、起動定義、永続化モデル |
| 状態モデル | `src/workspace/paneLayout.ts` ほか | 純粋関数（分割／集約／永続化／画面照合） |
| 画面マニフェスト | `src/workspace/manifests/*.json` | Herdr 移植の照合ルール |
| ワークスペース UI | `src/workspace/*.tsx` | サイドバー、タブ、分割ペイン、Files パネル |
| 端末サーフェス | `src/terminal/TerminalSurface.tsx` | xterm.js のライフサイクルと下端スナップショット |
| ファイルツリー | `src/FileExplorer.tsx` | ツリー、監視、Preview 連携 |
| プレビュー | `src/files/FilePreview.tsx` | ホストが返したプレビュー DTO の描画 |
| 設定 | `src/settings/*` | application／appearance |

### 4.2 ホストの所有範囲

| モジュール | 責務 |
| --- | --- |
| `src-tauri/src/lib.rs` | コマンド登録、更新、`shutdown_app`、レイアウト復旧 |
| `src-tauri/src/shells.rs` | シェル検出、実行ファイル検証、POSIX／PowerShell コマンド生成 |
| `src-tauri/src/workspaces.rs` | レジストリ移行、`workspace-layouts.json` の load/save/検証/復旧 |
| `src-tauri/src/terminal.rs` | PTY セッション、エージェントプリセット、世代検証、画面状態受付 |
| `src-tauri/src/file_manager.rs` | ワークスペースルート配下の list/preview/watch/open |
| `src-tauri/src/hook_ipc.rs` | ローカル IPC、256-bit トークン検証、世代照合 |
| `src-tauri/src/integrations.rs` | Hook／Plugin 資産の install/update/uninstall |

### 4.3 管理ランタイム（ホスト状態）

`manage` で保持する長寿命オブジェクト:

1. **`AppUpdateRuntime`** — アプリ内アップデータの状態。
1. **`TerminalRuntime`** — terminal id → PTY セッションの map（ペイン ID・世代を保持）。
1. **`WorkspaceWatcherRuntime`** — watch id → ファイル監視の map。
1. **`WorkspaceRuntime`** — レイアウト書込ガードと自動保存停止フラグ。
1. **`HookIpcRuntime`** — IPC リスナー、トークン。

______________________________________________________________________

## 5. ホスト–レンダラ契約

### 5.1 コマンド（invoke）

| グループ | 例 |
| --- | --- |
| ライフサイクル／更新 | `configure_native_titlebar`, `check_app_update`, `install_app_update` |
| ワークスペース | `workspace_list_roots`, `workspace_choose_root`, `workspace_add_root`, `workspace_remove_root` |
| レイアウト | `workspace_layout_load`, `workspace_layout_save`, `workspace_layout_backup_and_reset` |
| シェル／エージェント | `shell_list`, `agent_list_presets` |
| 端末 | `terminal_start`, `terminal_write`, `terminal_resize`, `terminal_stop` |
| 状態報告 | `agent_report_screen_state` |
| Hook IPC | `hook_ipc_initialize` |
| Integration | `integration_list`, `integration_install`, `integration_uninstall` |
| ファイル | `workspace_list_directory`, `workspace_preview_file`, `workspace_write_file`, `workspace_watch`, `workspace_unwatch`, `workspace_open_folder`, `workspace_inspect_attachment` |

エラーは新規コマンドで `{ code, message }`（`invalid_request`／`not_found`／`conflict`／`unavailable`／`io_error`／`invalid_config`／`stale_generation`）。

### 5.2 イベント（listen）

| イベント名 | ペイロードの目的 |
| --- | --- |
| `vintage://terminal-output` | PTY 出力チャンク（数値配列、世代付き） |
| `vintage://terminal-exit` | 終了コード／シグナル（世代付き） |
| `vintage://agent-activity` | エージェント活動状態（ペイン・世代・source・sessionId） |
| `vintage://workspace-changed` | 監視下の相対パス |
| `vintage://update-progress` | アップデータ進捗 |

### 5.3 Serde / TypeScript の整合

- 公開コマンド結果は Rust 側で `camelCase`、enum 値は `snake_case`。
- フィールド変更は **両側** が必要。挙動が変わるならテストも。

______________________________________________________________________

## 6. シェル検出と起動契約

- 標準シェルは安定 ID で保存し、実行のたびにパスを再解決する。カスタムシェルは正規化済み絶対パスを ID とし、ファイル存在・通常ファイル・Unix 実行ビット／Windows PATHEXT を検証する。
- Windows: PowerShell 7（PATH → `%ProgramFiles%\PowerShell\7`）優先、5.1（`%SystemRoot%` → PATH）フォールバック、Git Bash は `cmd\git.exe` 同一ルート検証（WSL／MSYS2 誤判定防止）。
- Unix: `$SHELL` → passwd ログインシェル → `/bin/bash` → `/bin/sh`。
- コマンド生成は純粋関数: POSIX 単一引用符、PowerShell `-EncodedCommand`（UTF-16LE Base64）。エージェント起動後は `exec bash --login -i` または `-NoExit` で同じシェルへ戻す。

______________________________________________________________________

## 7. エージェント起動と状態判定

- プリセット（Grok／Codex／Claude／OpenCode）は CLI 名で PATH 解決し、resume フラグ（`codex resume`／`claude --resume`／`opencode --session`）に対応。未検出でもプロファイル内 PATH を考慮して起動操作は許可し、spawn 失敗時に英語エラー。
- 画面判定は xterm.js の **ライブ下端 80 論理行**（スクロール位置や古い履歴は使わない）を、Herdr から移植したマニフェストで照合。`contains` は case-insensitive、`(?i)/(?m)` と `\u{...}` は JS 正規表現へ変換。
- **Hook／Plugin報告:** Settings → Integrations から管理資産を導入すると、OpenCode Pluginの活動状態報告（`opencode-plugin`）とCodex／Claude HookのセッションID報告（`runtime`、`state` なし）が `vintage://agent-activity` で届く。レンダラは source を判定し、`opencode-plugin`／`runtime` 報告を画面判定より優先してバッジへ反映する。セッションIDはサイドバーのペイン行に表示され、`session.deleted` などで `state` なし報告が来ると画面判定へ戻る。
  - Codexのインストールは `config.toml` の `[features] hooks = true` を保証し、`hooks.json` の `SessionStart` に管理エントリを追加する。**`codex exec`（非対話）ではSessionStartフックは実行されない**（実機確認済み）。フックは対話セッションでのみ発火する。
  - Claudeのインストールは `settings.json` の `hooks.SessionStart`（matcher `*`、timeout 10秒）に管理エントリを追加する。
  - OpenCodeは `plugins/vintage-agent-state.js` の配置だけで自動ロードされ、`opencode run` でライフサイクル報告が届く（実機確認済み）。
- 集約優先順位は `blocked > working > done > idle > unknown`。PTY `error` は blocked 相当＋別バッジ。

______________________________________________________________________

## 8. 永続化モデル

| ストア | 内容 |
| --- | --- |
| `working-directories.json` | `{ version: 1, roots: [{ id, path, title, createdAt }] }`。旧形式（パス配列・`[{path, createdAt}]`）は初回読込で移行 |
| `workspace-layouts.json` | `version: 1`。上限 64 ワークスペース／64 タブ／64 ペイン／分割深度 16。違反はファイル全体を拒否し、原本を変更せず自動保存停止、明示的な `Back up and reset layout` で復旧 |

実行状態（PTY ID、世代、scrollback、Hook トークン、プロンプト）は永続化しない。保存されたシェル ID が利用不能ならペインを Stopped のまま表示し、カスタム実行ファイルの消失時も自動置換しない。

______________________________________________________________________

## 9. セキュリティチェックリスト

- [ ] レンダラにエージェント用の生 FS／プロセス API がない
- [ ] 特権操作はすべて名前付きホストコマンド経由
- [ ] invoke/listen の文字列名はファサード 1 箇所が所有
- [ ] Hook 報告はトークン・ペイン ID・世代を検証
- [ ] IPC トークンがレンダラ向け型に現れない／ログに残らない
- [ ] ワークスペース操作はケイパビリティ・ハンドル + 相対パス + canonical 閉じ込め
- [ ] シェル実行ファイルを検証（実行ビット／PATHEXT）
- [ ] ID・寸法・入力サイズ・世代・引数上限の入力検証
- [ ] ペイン閉鎖とアプリ終了でのリソース解体（PTY・watcher・IPC）
- [ ] ログにプロンプト／Terminal 出力／秘密／IPC トークンを出さない
- [ ] レイアウト読込は全上限を検証し、違反で部分復旧しない

______________________________________________________________________

## 10. テスト戦略

| 層 | 何を試すか | どう試すか |
| --- | --- | --- |
| 純粋 TS モデル | 分割ツリー、状態集約、永続化検証、画面照合、PTY 世代 | `tests/*.test.ts`（Node テストランナー） |
| Rust 単体 | シェル検出全分岐、コマンド生成、レイアウト検証、Integration 冪等性 | 各モジュール内 `#[cfg(test)]` |
| 境界 | Rust DTO ↔ TS 型の同期 | 両側テストで固定 |

検証コマンド:

```bash
pnpm check          # フロントビルド + test:ui/workspace/agents + Rust tests/check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

______________________________________________________________________

## 11. 関連するプロジェクト規約

- [`AGENTS.md`](../AGENTS.md) — 所有マップ、ハード制約、検証コマンド、レビュー観点
- [`CONTRIBUTING.md`](../CONTRIBUTING.md) — 貢献フロー
- [`README.md`](../README.md) — 製品機能とインストール
- [`docs/multi-agent-terminal-implementation-plan.md`](multi-agent-terminal-implementation-plan.md) — 実装計画（Phase 0–8 と調査記録）

境界を変えるとき（新しい特権コマンド、新イベント、パス規則）は、実装と同時に `AGENTS.md` と本文書を更新し、アーキテクチャを願望ではなく実行可能な規約として保つ。

______________________________________________________________________

## 12. 用語集

| 用語 | 意味 |
| --- | --- |
| **Host（ホスト）** | 特権を持つ Rust プロセス（Tauri バックエンド） |
| **Renderer（レンダラ）** | webview UI（React） |
| **Workspace root** | ファイル操作用の canonical なディレクトリ・ケイパビリティ |
| **PTY** | 擬似端末。シェル／エージェントが動く実プロセス |
| **Generation（世代）** | ペインの起動ごとに増える番号。古いイベントを破棄する |
| **Screen manifest** | 端末下端の画面を活動状態へ写像するルール（Herdr 移植） |
| **Integration** | 各 CLI へ Hook／Plugin を導入・更新・削除する機能 |
