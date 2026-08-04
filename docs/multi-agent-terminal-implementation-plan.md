# Windows／Ubuntu対応マルチAIエージェントUI実装計画

## この計画で実現すること

このアプリを、Grok Build専用の会話クライアントからデスクトップ・ワークスペースへ変更する。複数のAIエージェントは、実際のTerminalプロセスとして並行実行する。

- 左側にワークスペース、タブ、エージェント状態を表示する。
- 中央にタブと再帰分割できるTerminalペインを配置する。
- 右側に既存のFile ExplorerとFile Previewを残す。
- Grok、Codex、Claude Code、OpenCode、任意コマンドを起動できるようにする。
- WindowsとUbuntuを初期リリースの正式対応環境にする。
- アプリ終了時にPTY（Pseudo Terminal、擬似端末）を停止し、画面配置だけを保存する。

実装担当は、本書に書かれた決定を前提に進める。「未解決事項の扱い」に該当する場合だけ、実装を止めて計画を更新する。

## 完了条件

- Windows PowerShell 5.1、PowerShell 7、Git for Windows付属`bash.exe`で対話Terminalを起動できる。
- Ubuntu 22.04 LTSと24.04 LTSで既定シェルを起動できる。
- 対応シェルからGrok、Codex、Claude Code、OpenCodeを起動できる。
- 複数のエージェントを別タブまたは分割ペインで同時に操作できる。
- CodexとClaude Codeは、CLIのライフサイクル時に呼ばれるHookからセッションIDを取得し、画面から活動状態を判定できる。
- OpenCodeは、CLIへ読み込むPluginの報告を活動状態の信頼できる情報源として扱える。
- File Explorer、File Preview、ファイル監視、ワークスペース境界検証が維持される。
- アプリ終了時に全PTY、子孫プロセス、ファイル監視、Hook用IPC（Inter-Process Communication、プロセス間通信）が停止する。
- 再起動後にワークスペース、タブ、分割配置がStopped状態で復元される。
- WindowsとUbuntuの継続的インテグレーションが通る。

## 対応範囲

### 初期リリースで対応する

- Windows 10 October 2018 Update以降とWindows 11のx64環境
- Ubuntu 22.04 LTSと24.04 LTSのx64環境
- Windows PowerShell 5.1
- PowerShell 7以降
- Git for Windows付属のGit Bash
- Ubuntu上のBash、Zsh、PowerShell 7、任意の実行可能シェル
- Grok、Codex、Claude Code、OpenCode
- 実行ファイルと引数配列で指定する任意コマンド
- タブ、左右分割、上下分割、境界リサイズ
- Hook／Pluginの明示的なInstall、Update、Uninstall

### 初期リリースでは対応しない

- アプリ終了後もPTYを動かすバックグラウンドサーバー
- SSH越しの接続、リモートワークスペース、複数クライアント接続
- tmuxまたはHerdrそのものの内部組み込み
- WSLの`bash.exe`と単体MSYS2を正式なGit Bashとして扱うこと
- Hook／画面判定マニフェストのリモート自動更新
- Terminal出力、プロンプト、ソースコードの永続化
- macOSの受け入れテスト。既存ビルドを意図的に壊さないが、初期保証には含めない
- パッケージ名`vintage`の変更

## 現行コードから再利用するもの

### Renderer

Rendererは、Tauri WebView内で動くReact UIを指す。

- `src/terminal/TerminalSurface.tsx`
  - xterm.jsの生成、入力、出力、リサイズ、フォーカス、破棄処理を再利用する。
  - 1コンポーネントが1PTYを所有する関係は維持する。
- `src/FileExplorer.tsx`
  - ワークスペースツリー、監視、ファイル選択を再利用する。
  - Composer廃止後は、添付ではなくPreviewを主操作にする。
- `src/files/FilePreview.tsx`
  - ホストが返すPreview DTO（Data Transfer Object、データ転送オブジェクト）の表示を維持する。
- `src/host/index.ts`
  - 生のTauriコマンド名とイベント名を集約する唯一のモジュールとして維持する。
- `src/layout/usePanelLayout.ts`
  - 左右パネルの幅、折りたたみ、キーボード操作を再利用する。

### Tauri host

Tauri hostは、OSのプロセス、ファイル、PTYを扱うRust側を指す。

- `src-tauri/src/terminal.rs`
  - `portable-pty`によるPTY作成、入出力、リサイズ、停止を拡張する。
  - 現在の既定シェル固定起動を、検証済み起動定義へ変更する。
- `src-tauri/src/file_manager.rs`
  - ワークスペース解決、正規化、シンボリックリンク脱出防止、Preview、監視を維持する。
- `src-tauri/src/lib.rs`
  - Tauriコマンド登録、アプリ終了時の全ランタイム停止、アプリデータの保存を再構成する。
- アプリ更新、外観、フォント設定は維持する。

## 撤去するGrok専用機能

- ACP（Agent Client Protocol）transportとイベント投影
- Grok Buildの認証、ログイン、ログアウト
- Grok利用量と課金表示
- 会話履歴、Composer、Permission Card、Prompt Queue
- モデル、推論量、承認モードの選択
- Grokセッション中心のサイドバーと検索
- `src/session/`配下のうちTerminalワークスペースで不要になる会話用ロジック
- `src-tauri/src/acp.rs`、`models.rs`、`usage.rs`の登録と参照

旧セッションJSONは自動削除しない。新UIでは読み込まず、ユーザーデータとして残す。

## 目標レイアウト

```text
┌────────────────┬──────────────────────────────────┬──────────────────┐
│ Workspaces     │ Tab: agents                      │ Files / Preview  │
│                │ ┌──────────────────────────────┐ │                  │
│ project-a      │ │ Codex / PowerShell           │ │ src/             │
│  agents        │ ├───────────────┬──────────────┤ │  App.tsx         │
│   Codex        │ │ Claude Code   │ OpenCode     │ │                  │
│   Claude       │ │ Git Bash      │ PowerShell 7 │ │ Preview          │
│   OpenCode     │ └───────────────┴──────────────┘ │                  │
│                │                                  │                  │
└────────────────┴──────────────────────────────────┴──────────────────┘
```

- 左サイドバーはワークスペース、タブ、ペイン、活動状態を階層表示する。
- 中央はタブごとに1つの分割ツリーを持つ。
- 右パネルはFile ExplorerとFile Previewを切り替える。
- 左右パネルは折りたたみと幅変更に対応する。
- ペインのフォーカス、タブ選択、分割境界はマウスとキーボードの両方で操作できるようにする。

## 状態モデル

### 永続化する状態

- `WorkspaceState`
  - `id`
  - 正規化済みワークスペースパス
  - 表示名
  - タブ一覧
  - 選択中タブID
- `AgentTabState`
  - `id`
  - 表示名
  - 分割ツリー
  - 選択中ペインID
- `PaneDefinition`
  - `id`
  - 表示名
  - シェルID
  - エージェント種別
  - 起動定義
  - 作業ディレクトリ
  - 任意のネイティブ・セッション参照
- Fileパネルの開閉状態と幅

### 永続化しない実行状態

- PTY IDとプロセスハンドル
- Terminal出力とスクロールバック
- Hook用IPCの接続先とトークン
- プロンプト、ソースコード、資格情報
- 実行中、待機中などの一時状態

### 分割ツリー

```ts
type PaneLayout =
  | { type: "leaf"; paneId: string }
  | {
      type: "split";
      direction: "horizontal" | "vertical";
      ratio: number;
      first: PaneLayout;
      second: PaneLayout;
    };
```

- `ratio`は0.2から0.8の範囲に制限する。
- ペインを閉じたら、片側だけになった分割ノードを残った子で置き換える。
- 不正な参照、重複ペインID、循環、17階層以上のツリーが1件でもあれば、ファイル全体の読み込みを拒否する。部分復旧はしない。
- 分割操作は純粋関数として実装し、ReactやTauriなしで単体テストできるようにする。

## シェル検出と起動契約

### 共通型

```ts
type ShellKind =
  | "powershell"
  | "pwsh"
  | "git-bash"
  | "bash"
  | "zsh"
  | "posix"
  | "custom";

interface ShellDescriptor {
  id: string;
  label: string;
  kind: ShellKind;
  executable: string;
  platform: "windows" | "unix";
  available: boolean;
  supportsAgentWrapper: boolean;
}
```

- 標準シェルは安定したIDで保存し、実行のたびにパスを再解決する。
- カスタムシェルだけは正規化した絶対パスを保存する。
- Rendererが渡した実行ファイルを無条件に信用しない。
- ホストはファイル存在と通常ファイルであることを検証する。Unixでは実行ビット、Windowsでは拡張子と`PATHEXT`も検証する。

### Windows PowerShell 5.1

- `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`を優先する。
- 見つからない場合だけ`PATH`上の`powershell.exe`を使う。
- `-NoLogo -NoExit`で起動する。
- ユーザーのプロファイルとPATHを使うため、通常起動では`-NoProfile`を付けない。

### PowerShell 7

- `PATH`上の`pwsh.exe`を最初に探す。
- 次に`%ProgramFiles%\PowerShell\7\pwsh.exe`を探す。
- PowerShell 7が見つかった場合はWindowsの既定シェルにする。
- 見つからない場合はWindows PowerShell 5.1へフォールバックする。

### Git Bash

- 以下の順でGit for Windows付属`bash.exe`を探す。
  - `PATH`上の`bash.exe`
  - `%ProgramFiles%\Git\bin\bash.exe`
  - `%ProgramFiles(x86)%\Git\bin\bash.exe`
  - `%LOCALAPPDATA%\Programs\Git\bin\bash.exe`
  - Settingsで指定された実行ファイル
- `PATH`候補は、同じGitルートに`cmd\git.exe`があることを確認する。
- WSLランチャーや無関係なMSYS2 BashをGit Bashとして自動選択しない。
- Portable Gitなど非標準配置はSettingsから登録できるようにする。
- 起動引数は`--login -i`とする。
- `CHERE_INVOKING=1`を設定し、選択したワークスペースからホームへ移動する挙動を抑止する。
- 親プロセスのPATHを維持する。
- `TERM=xterm-256color`と`COLORTERM=truecolor`を設定する。

### Ubuntu

- 有効な`$SHELL`を最初に使う。
- 次にpasswdデータベースのログインシェルを使う。
- どちらも使えない場合は`/bin/bash`、最後に`/bin/sh`へフォールバックする。
- Bash、Zsh、PowerShell 7、カスタム実行可能シェルを選択できるようにする。
- Windows固有処理は`cfg(windows)`で分離し、Unix側へコンパイルしない。

## エージェント起動契約

### 起動種別

```ts
type PaneLaunchSpec =
  | { type: "shell"; shellId: string }
  | {
      type: "agent";
      preset: "grok" | "codex" | "claude" | "opencode";
      shellId: string;
      args: string[];
      resumeSessionId?: string;
    }
  | {
      type: "custom";
      program: string;
      args: string[];
    };
```

- プリセットの実行ファイルと再開引数はRustホストが解決する。
- `agent`は指定シェル内で起動し、終了後に同じ対話シェルへ戻す。
- `custom`はシェルを介さず直接起動する。シェル構文が必要なら、`program`に`bash.exe`、`bash`、`pwsh.exe`などを明示する。
- Windowsの`.exe`は直接起動する。`.cmd`と`.bat`は`ComSpec /D /S /C`、`.ps1`は検出済みPowerShellの`-File`で起動する。
- パイプ、リダイレクト、複数コマンドが必要な場合だけ、ユーザーが`bash -lc`またはPowerShellを明示する。
- 上限は引数256個、1引数32 KiB、argv全体64 KiB、Terminal入力1回64 KiB、PTY寸法2〜500列／行とする。NUL文字を含む値は拒否する。

### PowerShell内での起動

- `powershell.exe -NoLogo -NoExit -EncodedCommand <base64>`または`pwsh.exe -NoLogo -NoExit -EncodedCommand <base64>`で起動する。起動スクリプトはUTF-16LEでBase64化する。
- Windows固有の引用符処理で引数が壊れないよう、専用のエスケープ関数を使う。
- エージェント終了後もPowerShellを閉じず、同じペインへ戻す。
- 生成したコマンド、引数、Hookトークンはログへ記録しない。

### Bash内での起動

- argv（実行ファイルと引数の配列）の各要素を、POSIX（Portable Operating System Interface）互換の単一引用符規則でエスケープする。
- `bash --login -i -c '<agent argv>; exec bash --login -i'`で起動し、エージェント終了後は同じBashへ戻す。
- シェルを維持するプリセット起動では、子エージェント単体の終了コードをUIへ公開しない。完了状態はHookまたは画面判定から得る。
- 空白、引用符、末尾バックスラッシュ、日本語を含む引数をテストする。

### ネイティブ・セッションの再開

- Codexは`codex resume <id>`を使う。
- Claude Codeは`claude --resume <id>`を使う。
- OpenCodeは`opencode --session <id>`を使う。
- Grok CLIは初期版でネイティブ・セッション再開を行わない。
- 復元後に自動再開しない。
- StoppedペインのRestartメニューから、新規起動または前回セッション再開を選べるようにする。

## Herdr調査から採用する状態判定

### 参照した実装

- 公式リポジトリ: `https://github.com/herdrdev/herdr`
- 調査基準コミット: `26a7bc860e4af516ef687d90b9f9dd2830f88d2a`
- 調査日: 2026-08-01
- 主な参照箇所
  - `src/integration/`
  - `src/integration/assets/codex/`
  - `src/integration/assets/claude/`
  - `src/integration/assets/opencode/`
  - `src/detect/manifests/`
  - `src/terminal/state.rs`

Herdrは、すべてのHookを同じ強さの情報源として扱っていない。イベントがライフサイクル全体を覆う場合だけ、Hook／Pluginを状態判定の唯一の信頼源にする。VINTAGEもこの分離を採用する。

### 状態の二層化

- PTY状態
  - `starting`
  - `running`
  - `stopped`
  - `exited`
  - `error`
- エージェント活動状態
  - `unknown`
  - `idle`
  - `working`
  - `blocked`
  - `done`

PTYが動いていることと、エージェントが作業中であることを同じ状態にしない。

### CodexとClaude Code

- Hookは`SessionStart`からネイティブ・セッションIDを取得するために使う。
- Hook報告で`working`、`blocked`、`idle`を確定しない。
- 権限キャンセル、Escape中断などHookで欠ける遷移があるため、PTY下端の画面判定を正とする。

### OpenCode

- 最初の有効報告後は、Pluginイベントを活動状態の信頼できる情報源にする。
- `session.status`、permission、question、tool、idleイベントを共通状態へ変換する。
- 子セッションのIDが親ペインのセッションIDを上書きしないようにする。
- 起動後5秒以内に有効報告がない場合、またはPlugin権限が失効した場合だけ画面判定へフォールバックする。

### 画面判定

- xterm.jsのライブ下端80論理行だけを使う。論理行は、画面幅による折返しを結合し、改行で確定した1行を指す。
- 入力上限は32 KiBとする。
- 出力受信後120 msでデバウンスする。
- ユーザーがスクロールしている表示位置や古い履歴を使わない。
- Codex、Claude Code、OpenCode、GrokのHerdrマニフェストを、優先度と条件を変えずJSONで同梱する。
- `blocked`は確認、権限、質問などの可視UIが一致した場合だけ付ける。
- OpenCode Pluginが活動状態の権限を持つ間は画面判定を適用しない。
- Herdr由来のルールを転用する場合はApache-2.0の出典と参照コミットを記録する。

### `done`の扱い

- 非表示ペインが`working`から`idle`または正常終了へ移った場合に`done`を生成する。
- ユーザーがそのペインを表示したら`idle`へ変更する。
- 集約優先順位は`blocked > working > done > idle > unknown`とする。
- ワークスペース状態は配下ペインの最も注意が必要な状態を表示する。

## Hook／Plugin報告のIPC

- Tauri hostが`127.0.0.1`の一時ポートでローカルIPCを待ち受ける。
- 起動ごとにWeb Cryptoで256-bitランダムトークンを生成する。
- 接続先、トークン、ペインID、世代番号をPTY子プロセスの環境変数だけへ注入する。
- Rendererへトークンを返さない。
- Hook／Plugin報告は以下を含む。
  - `paneId`
  - `generation`
  - `source`
  - `agent`
  - 任意の`state`
  - 任意の`sessionId`
  - 認証トークン
- ホストはトークン、ペインID、世代番号、状態値、16 KiBのメッセージ上限を検証し、受信時に連番を採番する。
- 古い世代の報告は破棄する。
- PTY終了時に、そのペインのHook権限とセッション関連の一時状態を解除する。
- 接続、読取、書込のtimeoutはそれぞれ500 msとする。
- Hookの失敗は呼出元へ伝播させず、エージェント処理を継続する。Integration診断画面には、秘密情報を含まない最終エラーだけを表示する。

### OS別Integration資産

- Windows
  - CodexとClaude CodeにはPowerShell `.ps1`を配置する。
  - Git Bashからエージェントを起動した場合もWindows用資産を使う。
- Ubuntu
  - CodexとClaude CodeにはPOSIX `.sh`を配置する。
- OpenCode
  - Node.js Pluginから共通IPCへ送信する。
- 全資産は、IPC環境変数がない通常のエージェント起動では何もしない。

## Integration管理

- SettingsにIntegrations画面を追加する。
- 各エージェントについて次を表示する。
  - CLIの検出状態
  - 使用可能なシェル
  - Hook／Pluginの配置先
  - `Not installed`、`Installed`、`Outdated`
  - Install、Update、Uninstall
- グローバル設定の変更は、ユーザーがボタンを押した場合だけ実行する。
- Codexでは`CODEX_HOME`、Claude Codeでは`CLAUDE_CONFIG_DIR`を尊重する。
- 管理資産には識別子と版数を埋め込む。
- 自分が追加した設定エントリだけを変更または削除する。
- 不正なJSON／TOMLは書き換えず、対象パス付きの英語エラーを返す。
- 管理マーカーのない同名ファイルは上書きしない。
- 候補を同じディレクトリの一時ファイルへ書き、構文と管理対象を検証してから置換する。
- Uninstall後も、ユーザーが作成したHook、Plugin、設定、資格情報は保持する。

## Host／Renderer境界

### Rustホストが所有する

- シェルとエージェント実行ファイルの検出
- シェル別コマンド生成と引数エスケープ
- PTYの作成、入出力、リサイズ、停止
- Hook用IPCと認証トークン
- Integrationのファイル変更
- ワークスペース境界内のファイル操作
- レイアウトの読み書き
- アプリ終了時の全ランタイム停止

### React Rendererが所有する

- ワークスペース、タブ、分割ペインの表示
- xterm.jsの描画とライブ下端スナップショット
- 純粋な画面マニフェスト照合
- マウスとキーボードの操作状態
- File ExplorerとPreviewの表示
- ホストが返した状態の集約表示

### 禁止事項

- Reactからプロセスを直接起動しない。
- Reactからファイルシステムへ直接アクセスしない。
- 生のTauriコマンド名をコンポーネントへ散在させない。
- Rendererが渡した絶対パスをワークスペースとして信用しない。
- Prompt、Terminal出力、ソースコード、資格情報、IPCトークンをログへ残さない。
- ユーザー向けUIとホスト由来のエラーは英語で記述する。

## Tauri契約

### コマンド

- シェル一覧と利用可否の取得
- シェル実行ファイルの検証
- エージェントプリセットの検出
- PTYのstart、write、resize、stop
- レイアウトのload、save
- Integrationのstatus、install、update、uninstall
- Rendererで判定した画面状態の報告

### イベント

- PTY output
- PTY exit
- エージェント活動状態の変更
- シェル検出状態の変更
- Integration状態の変更

### 同期規則

- Rustの公開DTOは`camelCase`でシリアライズする。
- TypeScript型とRust DTOを同じ変更で更新する。
- raw command名とevent名は`src/host/index.ts`だけが持つ。
- ペインID、世代番号、Terminal IDの対応はホスト側を正とする。

## 永続化

- アプリデータに`workspace-layouts.json`を追加する。
- 一時ファイルへの書込、JSON再読込、同一ディレクトリ内renameの順で更新する。
- 不正ファイルは「永続化の上限と復旧」に定義した明示的な復旧フローで扱う。
- Windowsパスは文字列連結せず`PathBuf`で処理する。
- 保存されたシェルIDが利用不能なら、ペインをStoppedのまま表示して再選択を促す。
- 保存されたカスタム実行ファイルが消えていても、自動的に別プログラムへ置き換えない。
- レイアウト読込時は「永続化の上限と復旧」に定義した全上限を検証する。

## 実装判断を残さないための固定契約

### ワークスペースの信頼源

- Grokセッション撤去後は、Rustホストが管理する`working-directories.json`をワークスペース境界の信頼源にする。
- 既存のパス配列は初回読込時にUUIDを割り当て、`{ id, path, title }`へ移行して同じファイルへ保存する。
- File APIはRendererから絶対パスを受け取らず、`workspaceId`と正規化済み相対パスを受け取る。
- `workspace_list_roots`、`workspace_choose_root`、`workspace_add_root`、`workspace_remove_root`を公開する。
- 削除は登録解除だけを行い、実ディレクトリとファイルを削除しない。
- 登録解除時は対象ワークスペースのPTYとwatcherを停止してから、レイアウトをStoppedとして残す。

### 永続化の上限と復旧

- `workspace-layouts.json`のルートへ`version: 1`を必須とする。
- 上限は64ワークスペース、1ワークスペース64タブ、1タブ64ペイン、分割深度16とする。
- 表示名は128 Unicode scalar values、プログラムは32 KiB、引数は256個、1引数32 KiB、argv全体64 KiBを上限とする。
- 構文不正、version欠落、未知のversion、上限違反、不正参照が1件でもあれば、ファイル全体の読み込みを拒否する。部分復旧はしない。
- 読み込み失敗時は原本を変更せず、空レイアウトをメモリ上だけで表示し、自動保存を停止する。
- UIへ`Back up and reset layout`を表示する。ユーザーが実行した場合だけ、原本を`workspace-layouts.invalid-<UTC timestamp>.json`へrenameしてversion 1の空ファイルを保存する。
- version 1が初版なので、自動マイグレーションは持たない。将来は`vN -> vN+1`の純粋関数を順番に適用する。

### CLI検出と実行結果

- ホストによるCLI検出は、候補表示と警告のための補助情報とする。
- ユーザープロファイル読込後だけPATHへ追加されるCLIがあるため、未検出でも起動操作を許可する。
- 起動成否はPTY spawnの結果で確定し、失敗時はプログラム名とシェル名を含む英語エラーを返す。引数と環境変数はエラーへ含めない。
- Grok CLIは実行ファイル名`grok`を使う。初期版ではHookとセッション再開を提供せず、Herdrの`grok.toml`を移植した画面判定だけを使う。

### Integrationの配置と所有権

Integrationは、各CLIへHookまたはPluginを導入し、版数確認、更新、削除を行う機能を指す。

- 管理資産の先頭へ`VINTAGE_INTEGRATION_ID=<agent>`と`VINTAGE_INTEGRATION_VERSION=1`をコメントとして入れる。
- Codex
  - ルートは`CODEX_HOME`、未設定時は`~/.codex`とする。
  - `vintage-agent-state.ps1`または`vintage-agent-state.sh`を配置する。
  - `hooks.json`の`SessionStart`へ、管理スクリプトを引数`session`で呼ぶエントリを追加する。
  - `config.toml`の`[features] hooks = true`を保証する。他のfeatureは保持する。
  - stdin JSONの`hook_event_name == "SessionStart"`、`session_id`、`transcript_path`を検証し、セッションIDだけを報告する。
- Claude Code
  - ルートは`CLAUDE_CONFIG_DIR`、未設定時は`~/.claude`とする。
  - `hooks/vintage-agent-state.ps1`または`hooks/vintage-agent-state.sh`を配置する。
  - `settings.json`の`hooks.SessionStart`へmatcher `*`、timeout 10秒の管理コマンドを追加する。
  - stdin JSONの`hook_event_name == "SessionStart"`、`session_id`、`transcript_path`を検証する。`agent_id`がある子エージェント報告は無視する。
- OpenCode
  - ルートは`XDG_CONFIG_HOME/opencode`、未設定時は`~/.config/opencode`とする。
  - `plugins/vintage-agent-state.js`を配置し、他のPluginファイルは変更しない。
  - `session.created`と`session.updated`はセッション識別、`session.status`はidle／working、permission／question要求はblocked、応答とtoolイベントはworking、`session.idle`はidleへ変換する。
  - `parentID`がある子セッションは親ペインのセッションIDを上書きしない。
- WindowsのHookコマンドは`powershell.exe -NoProfile -ExecutionPolicy Bypass -File <script> session`とする。
- UbuntuのHookコマンドは`sh <script> session`とする。
- Uninstallは管理マーカーと正規化済みコマンドが一致するエントリだけを削除する。
- 同名ファイルに管理マーカーがない場合は`conflict`を返し、上書きも削除もしない。

### Hook IPCの固定値

- `std::net::TcpListener`を専用threadで動かし、Tokioの`net` featureと新規依存を追加しない。
- 256-bitトークンはRendererのWeb Crypto `crypto.getRandomValues`で生成し、初期化コマンドで一度だけホストへ渡す。Rendererは初期化後に値を保持せず、ホストはログと永続化へ出さない。
- 脅威モデルは、同一端末上の別プロセスによる偽報告を防ぐこととする。WebView侵害はTauri invokeを実行できるため、このトークンの防御対象外とする。
- 接続timeoutは500 ms、読取timeoutは500 ms、書込timeoutは500 ms、1報告の上限は16 KiBとする。
- 送信側`seq`は使わない。ホストが有効報告の受信順に、ペイン世代ごとの64-bit連番を採番する。
- OpenCode PluginはPromise chainで送信順を直列化する。Codex／Claude Hookはセッション識別専用なので、同一世代の最後に受信した有効セッションIDを採用する。
- OpenCode Pluginの状態権限は最初の有効状態報告で開始し、PTY終了、世代変更、`session.deleted`、または最終報告から30秒経過した時点で失効する。
- 起動後5秒以内にOpenCodeの有効報告がなければ、画面判定を継続する。

### 画面マニフェストの移植範囲

- Herdrコミット`26a7bc8`の`codex.toml`、`claude.toml`、`opencode.toml`、`grok.toml`を意味変更せずJSONへ移植する。
- 優先度、region、AND／OR／NOT条件、正規表現、visible flagsを保持し、実装者がルールを選別しない。
- 入力はxterm.jsのactive buffer下端80論理行とする。論理行は画面幅による折返しを結合し、改行で確定した行を指す。
- ANSI制御列を除去し、Unicodeを保持し、末尾空白だけを削除する。英字はルールが明示した場合だけcase-insensitiveで比較する。
- 集約優先順位は`blocked > working > done > idle > unknown`とする。
- PTY `error`は別バッジを表示し、ワークスペース集約では`blocked`相当とする。
- `stopped`と正常`exited`は、直前の活動状態が`done`でない限り集約対象から外す。
- 状態値はwireと保存データで小文字、UIラベルで先頭大文字に統一する。

### 最小Tauri wire契約

```ts
interface StartTerminalRequest {
  terminalId: string;
  paneId: string;
  generation: number;
  workspaceId: string;
  launch: PaneLaunchSpec;
  cols: number;
  rows: number;
}

interface TerminalInfo {
  terminalId: string;
  paneId: string;
  generation: number;
  workingDirectory: string;
  shell: ShellDescriptor;
  processId: number;
}

interface TerminalOutputEvent {
  terminalId: string;
  generation: number;
  data: number[];
}

interface TerminalExitEvent {
  terminalId: string;
  generation: number;
  exitCode: number | null;
  signal: string | null;
}

interface AgentActivityEvent {
  paneId: string;
  generation: number;
  activity: "unknown" | "idle" | "working" | "blocked" | "done";
  source: "screen" | "opencode-plugin" | "runtime";
  sessionId: string | null;
}
```

- コマンド名は`terminal_start`、`terminal_write`、`terminal_resize`、`terminal_stop`を維持する。
- 新規コマンド名は`workspace_layout_load`、`workspace_layout_save`、`shell_list`、`agent_list_presets`、`integration_list`、`integration_install`、`integration_uninstall`、`agent_report_screen_state`、`hook_ipc_initialize`とする。
- エラーは`{ code, message }`へ統一し、codeは`invalid_request`、`not_found`、`conflict`、`unavailable`、`io_error`、`invalid_config`、`stale_generation`とする。
- Rust DTOは`camelCase`、enum値は`snake_case`をwire形式とする。

### プロセスツリーの停止

- UbuntuではPTY子を独立process groupで起動する。停止時はgroupへSIGTERMを送り、500 ms後も残る場合はSIGKILLを送る。
- WindowsではPTY子のPIDへ`taskkill.exe /PID <pid> /T /F`をシェルを介さず実行し、完了後に`ChildKiller`を呼ぶ。
- PIDは数値型で保持し、文字列入力からtaskkill対象を作らない。
- ペイン閉鎖、ワークスペース登録解除、アプリ終了の全経路で同じ停止関数を使う。

## 実装順序

### Phase 0: 外部契約と作業ツリーを固定する

- `git status --short`を記録し、既存変更の所有者を変更しない。
- 実装環境にあるCodex、Claude Code、OpenCode、Grokの版数とHook／Plugin設定形式を公式資料または実CLI出力で再確認する。
- Herdrコミット`26a7bc8`との差分を本書の末尾へ記録し、非互換があれば実装前に本書を更新する。
- WindowsとUbuntuで利用する`portable-pty 0.9`の起動・停止境界を確認する。

#### 完了条件

- 対象CLIごとに確認した版数、設定ファイル、イベント名、差分が記録されている。
- 既存の未コミット変更を上書きしない作業範囲が確定している。

### Phase 1: 純粋な状態モデルを先に作る

- ワークスペース、タブ、ペイン、分割ツリーの型を追加する。
- 分割、閉鎖、選択、状態集約、復元検証を純粋関数で実装する。
- 既存UIへ接続する前に単体テストを通す。

#### 完了条件

- ReactとTauriなしで、分割ツリーと状態集約の全テストが通る。

### Phase 2: ワークスペース登録とレイアウト永続化を作る

- 既存`working-directories.json`をID付きレコードへ移行する。
- `workspace-layouts.json`のversion 1、load、save、検証、明示的な復旧を実装する。
- File APIを`workspaceId`基準へ変更し、絶対パスをRenderer契約から外す。
- 保存と復元の純粋テストをUI接続前に通す。

#### 完了条件

- 正常ファイルは同一レイアウトへround-tripする。
- 破損、未知version、上限違反では原本が変更されず、自動保存が停止する。
- 既存ワークスペース登録が失われずID付き形式へ移行される。

### Phase 3: シェル検出とPTY契約を拡張する

- WindowsとUnixのシェル解決を分離する。
- PowerShell 5.1、PowerShell 7、Git Bash、Ubuntu既定シェルを検出する。
- `terminal_start`へ起動定義、世代番号、検証済み環境変数を追加する。
- プラットフォーム別コマンド生成を単体テストする。

#### 完了条件

- Ubuntu上の自動テストでWindowsとUnixの全分岐を検証できる。
- Windows実機で3種類の対応シェルがPTY上で起動する。

### Phase 4: 中央のTerminalワークスペースを作る

- `src/App.tsx`から会話UIへの依存を段階的に外す。
- 左サイドバー、タブ、再帰分割、ペイン操作を接続する。
- xterm.jsインスタンスとPTY寿命を各leafペインへ対応させる。
- ペイン閉鎖時にPTYを停止してから状態から削除する。

#### 完了条件

- 4ペイン以上を作成し、個別に入力、リサイズ、閉鎖できる。
- タブ切替で非表示になったPTYが意図せず停止しない。

### Phase 5: File Explorerを右パネルへ移す

- File ExplorerとPreviewをTerminalセッションから独立させる。
- Composer添付依存を削除する。
- ワークスペース変更時に監視対象とPreviewを更新する。

#### 完了条件

- TerminalとFile Previewを同時表示できる。
- Filesパネルを閉じたときにファイル監視が停止する。

### Phase 6: エージェント起動と状態判定を追加する

- プリセット起動、カスタム起動、シェル復帰を実装する。
- xterm.js下端スナップショットと画面マニフェストを接続する。
- `done`確認とサイドバー集約を追加する。

#### 完了条件

- Codex、Claude Code、OpenCodeのworking／blocked／idleを表示できる。
- 未検出CLIには英語の警告を表示するが、プロファイル内PATHを考慮して起動操作は許可する。

### Phase 7: Hook／Plugin連携を追加する

- ローカルIPC、世代番号、`seq`、トークン検証を実装する。
- Windows用PowerShell資産、Ubuntu用Shell資産、OpenCode Pluginを追加する。
- Settingsへ明示的な管理操作を追加する。

#### 完了条件

- Codex／Claude CodeのセッションIDを取得できる。
- OpenCode Plugin報告が画面判定より優先される。
- 古いペインから届いた遅延報告が無視される。

### Phase 8: Grok専用UIを撤去して文書を同期する

- 未参照になった会話、ACP、認証、利用量コードとテストを削除する。
- README、`AGENTS.md`、`docs/architecture.md`を新構成へ更新する。
- 既存の未コミット変更と競合する場合は、内容を保持して手動統合する。

#### 完了条件

- Grok専用TauriコマンドとRenderer参照が残っていない。
- 文書のコマンド、パス、対応OSが実装と一致する。

## テスト計画

### TypeScript単体テスト

- 左右・上下の再帰分割
- 分割比率の0.2〜0.8制限
- ペイン閉鎖時の縮約
- タブとペインIDの整合性
- 不正・過深な永続化データの拒否
- `blocked / working / done / idle / unknown`の集約
- `done`の確認済み化
- Codex、Claude Code、OpenCodeの画面マニフェスト
- スクロール位置でなくライブ下端を使うこと
- Fileパネルとワークスペース変更

### Rust単体テスト

- PowerShell 7と5.1の検出優先順位
- Git Bashの既知パス、PATH、カスタムパス
- WSLの`bash.exe`をGit Bashと誤判定しないこと
- Ubuntuの`$SHELL`、passwd、`/bin/bash`、`/bin/sh`フォールバック
- PowerShell EncodedCommandの復号後argv
- POSIX引用の復元
- 空白、日本語、引用符、末尾バックスラッシュを含むパスと引数
- cwd、ID、寸法、入力、引数、環境変数の上限
- PTY終了、ペイン閉鎖、アプリ終了時の子孫プロセス停止
- IPCトークン、世代番号、ホスト採番、サイズ制限
- OpenCode Plugin権限と画面フォールバックの排他
- Codex／Claude Code Hookが活動状態を上書きしないこと
- IntegrationのInstall、Update、Uninstall、冪等性
- 不正設定と管理外ファイルの保護
- レイアウトの原子的保存とStopped復元

### package.jsonのテストスクリプト

- `test:workspace`: `tests/paneLayout.test.ts`、`workspacePersistence.test.ts`、`workspaceRegistry.test.ts`
- `test:agents`: `tests/agentState.test.ts`、`screenDetection.test.ts`、`shellLaunch.test.ts`、`integrationState.test.ts`
- `test:ui`: 維持するappearance／fontScaleテストと、新しいFileパネル状態テスト
- `test`: `test:workspace`、`test:agents`、`test:ui`を順に実行する。
- `check`: `pnpm build && pnpm test && pnpm test:rust && pnpm check:rust`とする。
- 旧Grok会話用`test:session`と、会話turn用`test:timing`は対象コード撤去と同じPhaseで削除する。

### CI

- GitHub Actionsを`windows-latest`、`ubuntu-22.04`、`ubuntu-24.04`で実行する。
- 両OSで以下を実行する。
  - `pnpm build`
  - `pnpm test`
  - `cargo test --manifest-path src-tauri/Cargo.toml`
  - `cargo check --manifest-path src-tauri/Cargo.toml`
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Windows CIではPowerShell PTYの起動、入出力、終了を自動確認する。Windows 10実機確認は各リリースの必須手動ゲートとする。
- Git BashがRunnerにある場合は`bash.exe`のPTYスモークテストを行う。
- RunnerにGit Bashがない場合も、パス検出とコマンド生成の単体テストは必須にする。

### 手動受け入れテスト

- Windows PowerShell 5.1でcwd、Unicode、ANSI、リサイズ、`Ctrl+C`を確認する。
- PowerShell 7で同じ項目を確認する。
- `C:\Program Files\Git\bin\bash.exe`でGit Bashを起動する。
- 空白と日本語を含むWindowsワークスペースをGit Bashで開く。
- PowerShellとGit Bashの両方からCodex、Claude Code、OpenCodeを起動する。
- エージェント終了後に元のシェルへ戻ることを確認する。
- Hook／Pluginによるセッション識別と状態表示を確認する。
- Ubuntu 22.04と24.04でBashと各エージェントを確認する。
- 両OSでFile Explorer、Preview、ファイル監視を確認する。
- アプリ再起動後に配置だけが復元され、プロセスが自動起動しないことを確認する。

## 必須検証コマンド

アプリケーションコードを変更した段階では、少なくとも以下を実行する。

```bash
pnpm check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

対象CLIやWindows実機が使えない場合は、実行できなかった確認を明示する。自動テストが通ったことを、実機確認済みと読み替えない。

## 実装時の注意

- 着手時に`git status --short`を確認し、ユーザーが所有する既存変更を削除、reset、checkout、上書きしない。
- `docs/architecture.md`は、Host／Renderer境界を変更する段階で本計画と同期する。
- lockfileは、ユーザーから明示的な許可がない限り変更しない。
- 新規依存を増やす前に、標準ライブラリ、既存の`portable-pty`、`serde_json`、`base64`で実装できないか確認する。
- ユーザー向けUIとホスト由来のエラーは英語に統一する。
- 実装途中で見つけた問題は、同じ変更内で直すか、本書の未解決事項へ根拠付きで記録する。

## 未解決事項の扱い

現時点で実装担当へ委ねる製品判断はない。次の場合だけ計画変更として扱う。

- `portable-pty 0.9`では正式対応Windows上の要件を満たせないことが再現できた場合
- Git Bashから対象エージェントを起動するために、既存依存だけでは安全なargv変換ができない場合
- 各CLIのHook／Plugin契約が調査基準コミット以降に非互換変更された場合
- Tauriのライフサイクル上、アプリ終了時の全PTY停止を保証できない場合

変更が必要なときは、次を本書へ追記してから実装方針を変える。

- 正確な再現手順
- 対象OSとCLIバージョン
- エラーまたは不一致の証拠
- 採用する代替案
- Windows／Ubuntu互換性への影響

## Phase 0 調査記録（2026-08-01実施）

### 作業ツリーの所有者

`git status --short` の記録（ベースコミット `1719e7c`）。

- ` M AGENTS.md`、` M README.md` — ユーザー所有の既存変更。実装中は変更せず、Phase 8で内容を保持して手動統合する。
- `?? .firecrawl/`、`?? data/`、`?? docs/architecture.md`、`?? docs/multi-agent-terminal-implementation-plan.md` — 未追跡ファイル。削除・上書きしない。

### 対象CLIの実機版数（Ubuntu x64実機）

- codex-cli 0.146.0 — `codex resume [SESSION_ID]` の存在を確認。
- Claude Code 2.1.220 — `claude --resume <id>` の存在を確認。
- OpenCode 1.18.8 — `opencode -s/--session <id>` の存在を確認。
- Grok 0.2.118 — 起動を確認。計画どおりネイティブ再開は対象外。

### Hook／Plugin契約の実機確認

- Codex: `~/.codex/hooks.json` で `SessionStart` を含む11イベント（SessionStart、SessionEnd、UserPromptSubmit、PreToolUse、PostToolUse、PermissionRequest、Stop、PreCompact、PostCompact、SubagentStart、SubagentStop）を確認。stdin JSON は `hook_event_name`、`session_id`、`source` を含む（実稼働中の統合スクリプトで確認）。`config.toml` の `[features] hooks = true` を確認。
- Claude Code: `settings.json` の hooks 構造 `{ "<Event>": [{ matcher, hooks: [{ type: "command", command, timeout }] }] }` を実機で確認。
- OpenCode: Plugin は `~/.config/opencode/plugins/*.js` のnamed export。イベント `permission.asked`、`question.asked`、`permission.replied`、`question.replied`、`question.rejected`、`session.created`、`session.updated`、`session.status`（busy/retry/idle）、`session.idle` を実機Pluginで確認。`session.deleted` と tool イベント（`tool.execute.before/after`）は Phase 7 で OpenCode のPlugin型定義により最終確認する。

### Herdr 調査基準コミット 26a7bc8 の確認

参照マニフェストを `.firecrawl/herdr-manifest-{codex,claude,opencode,grok}.toml` に取得済み。

- `codex.toml`: version 2026.07.18.1、min_engine_version 2（78行）
- `claude.toml`: version 2026.07.13.1、min_engine_version 2（158行）
- `opencode.toml`: version 2026.06.10.1、min_engine_version 1（37行）
- `grok.toml`: version 2026.07.16.2、min_engine_version 3（164行）

調査基準日と同日のため差分なし。Phase 6 の移植はこれらを正とする。

計画に対する VINTAGE 拡張（非互換ではなく追加）: OpenCode Plugin の `parentID` 子セッション保護、`session.deleted` での権限解放、tool イベント → working 変換、30秒の権限失効、起動後5秒の猶予。実機Pluginにない要素は Phase 7 でこの拡張どおりに実装する。

### portable-pty 0.9.0 の境界（クレート実機ソースで確認）

- Unix: 子プロセス起動時に `setsid()` + `TIOCSCTTY`（`src/unix.rs:257,271`）。独立 process group と制御端末が確保されるため、計画の SIGTERM → 500 ms → SIGKILL グループ停止は `process_group_leader()` または子PIDから実装可能。
- Windows: ConPTY 実装（`src/win/psuedocon.rs`、`CreatePseudoConsole`）。ConPTY は Windows 10 1809 以降で利用可能であり、計画の最小OS要件と一致。
- 計画変更が必要な非互換は確認されなかった。
