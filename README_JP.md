<p align="center">
  <img src="./assets/readme/hero.svg" width="100%" alt="複数のコーディングエージェントを実ターミナルで並行実行し、対応が必要なエージェントを把握できるVINTAGE">
</p>

<p align="center">
  <strong>Visual Interface for Terminal Agents.</strong><br>
  An open-source multi-agent desktop workspace.
</p>

<p align="center">
  <strong>WindowsとLinuxで使える、オープンソースのマルチエージェント・ターミナルワークスペース。</strong>
</p>

<p align="center">
  <strong>日本語</strong> · <a href="./README.md">English</a>
</p>

<p align="center">
  <a href="https://github.com/Tomatio13/vintage/releases/latest"><strong>VINTAGEをダウンロード</strong></a> ·
  <a href="#使い始める">使い始める</a> ·
  <a href="#vintageでできること">機能</a> ·
  <a href="CONTRIBUTING.md">コントリビューション</a>
</p>

> [!WARNING]
> VINTAGEは開発初期のリリースです。機能、対応環境、保存される設定は、今後のリリースで変更される場合があります。

> [!IMPORTANT]
> VINTAGEは独立した非公式プロジェクトです。対応するエージェントCLIの開発元とは提携しておらず、承認も受けていません。

## すべてのエージェントを、ひとつの画面で

VINTAGEは、各コーディングエージェントに実ターミナルを割り当て、その活動状況をワークスペースへ集約します。Codex、Claude Code、OpenCode、または任意のシェルコマンドを並行実行し、対応が必要なタブやペインへすぐに移動できます。

<p align="center">
  <img src="./docs/assets/vintage-main.png" width="100%" alt="ワークスペースのサイドバー、分割ターミナル、ファイルプレビューを表示したVINTAGEのデスクトップ画面">
</p>

## VINTAGEでできること

- **実ターミナルを使えます。** 各ペインは対話可能な擬似端末（PTY）シェルを持ち、入力、サイズ変更、コピー／ペースト、IME、スクロールバック、明示的な終了に対応します。
- **並行作業を整理できます。** 名前を変更できるタブと再帰的に分割できるペインを使い、同じ登録済みプロジェクトフォルダで複数のエージェントを動かせます。
- **対応が必要な作業を見つけられます。** Codex、Claude Code、OpenCodeの管理されたIntegrationが、認証付きローカルHook IPCを通じて活動状態を報告します。状態はペイン見出し、タブ、サイドバーへ表示されます。
- **作業ファイルを近くに置けます。** ワークスペース見出しのFilesからパネルを開き、境界をドラッグして幅を変え、登録済みワークスペースのツリーを閲覧して対応ファイルを安全にプレビューできます。
- **操作を調整できます。** Graphite、Dark、Light、Systemの配色、ターミナルフォント、スクロールバック、既定シェル、ショートカットを設定できます。
- **ネイティブで動作します。** v0.5で旧TauriアプリからGPUIデスクトップアプリへ移行しました。GPUI版が唯一のVINTAGEデスクトップ版です。

## 現在の対応状況

- **プラットフォーム:** Linux x86-64、Windows x86-64向けパッケージ。
- **ターミナル:** 対話シェルのペイン、タブ、右／下への再帰分割、境界のサイズ変更、タブ名変更、終了操作。
- **ワークスペースファイル:** 登録済みワークスペースの閲覧と、サイズを制限した読み取り専用プレビュー。Filesパネルは任意のファイルシステムパスやリモートURLを受け取りません。
- **エージェント:** Codex、Claude Code、OpenCodeの管理されたIntegrationと、ターミナルシェルから実行する任意のコマンド。
- **活動状態:** 認証済みHook IPCがペインごとの状態を報告します。**Settings → Integrations**から管理資産を導入して、新しいエージェントセッションを開始してください。
- **設定:** Graphite、Dark、Light、Systemの配色、フォント、既定シェル、スクロールバック、変更可能なショートカット。

## VINTAGEをインストール

VINTAGEは、コンピューターにインストール済みのエージェントCLIを直接実行します。使うCLIを準備してから、OSに合ったVINTAGEのパッケージをインストールしてください。認証情報とログインは各CLIが管理します。

### 1. エージェントCLIを準備する

利用するエージェントを各公式手順でインストールし、`PATH`から実行できることを確認します。

```bash
codex --version     # OpenAI Codex
claude --version    # Claude Code
opencode --version  # OpenCode
```

### 2. VINTAGEをダウンロードする

公式の[GitHub Releasesページ](https://github.com/Tomatio13/vintage/releases/latest)を開き、**Assets**から環境に合ったファイルを選びます。

- **Windows x86-64:** `VINTAGE_*_x64-setup.exe`
- **Debian／Ubuntu x86-64:** `VINTAGE_*_amd64.deb`
- **その他のLinux x86-64:** `VINTAGE_*_amd64.AppImage`

#### Windows

`VINTAGE_*_x64-setup.exe`を実行し、案内に従ってください。続行前に、取得元が`github.com/Tomatio13/vintage`であることを確認してください。

#### Debian／Ubuntu

```bash
sudo apt install ./VINTAGE_*_amd64.deb
```

#### その他のLinux

```bash
chmod +x VINTAGE_*_amd64.AppImage
./VINTAGE_*_amd64.AppImage
```

## 使い始める

1. ワークスペースのサイドバーからプロジェクトフォルダを開きます。登録したフォルダが、ターミナルとFilesパネルの信頼できる起点になります。
1. ターミナルを起動します。新しいターミナルでは、選択した既定シェルを使います。
1. シェルで`codex`、`claude`、`opencode`などを直接実行します。
1. ワークスペース見出しの分割操作またはショートカットで、作業を並行させます。
1. ワークスペース見出しの**Files**を開き、プロジェクトを閲覧・プレビューします。
1. ワークスペース画面に活動状態を表示したい場合は、**Settings → Integrations**から任意のエージェントHookを導入します。

既定ショートカットは**Settings → Shortcuts**で変更できます。

- `Ctrl+Shift+N`: 新しいターミナル
- `Ctrl+Shift+D`: 右へ分割
- `Ctrl+Shift+T`: 下へ分割
- `Ctrl+Shift+W`: ペインを閉じる
- `Ctrl+Shift+C` / `Ctrl+Shift+V`: コピー／ペースト

VINTAGEの終了時には、ターミナルセッションとローカルHook IPCサービスを停止します。終了後にエージェントプロセスが自動再起動することは想定しないでください。

## 仕組み

```text
GPUIデスクトップビュー
      │ 検証済みのワークスペース／ターミナル要求
      ▼
VINTAGEランタイムサービス
      │ PTY、ファイルサービス、Hook IPC
      ▼
エージェントCLIを実行するシェルプロセス
```

GPUIビューがプロセスを直接起動したり、任意のファイルを直接読んだりすることはありません。ランタイムサービスが、シェル検出、実行ファイルの検証、ターミナルのライフサイクル、登録済みワークスペースへのファイルアクセス、プレビュー、設定の保存、ローカルHook IPCを担当します。

各ターミナルペインは、対話PTYシェルを実行します。ペイン、タブ、ワークスペース、アプリケーションを閉じると、ランタイムは対応するセッションを停止します。ファイル操作は登録済みワークスペースから解決され、トラバーサル、シンボリックリンク経由の逸脱、通常ファイル以外のプレビュー対象、表示上限を超えるデータを拒否します。

詳しい所有権と信頼境界は、[docs/architecture.md](docs/architecture.md)を参照してください。

## エージェントHookの設定

VINTAGEは、Codex、Claude Code、OpenCodeから管理されたHookまたはプラグイン資産を通じて活動状態を受け取れます。

1. **Settings → Integrations**を開きます。
1. 使うエージェントで**Install**を選びます。
1. VINTAGEのターミナルペインで、新しい対話エージェントセッションを開始します。

VINTAGEは自分が所有する設定エントリとファイルだけを追加、更新、削除します。既存の競合資産は上書きせず、状態として表示します。ローカル報告経路は認証付きのloopback専用で、トークンは保存もログ出力もしません。

## プライバシーとセキュリティ

- VINTAGEは、エージェントの認証情報やブラウザー認証トークンを読み取ったり保存したりしません。
- ターミナル出力、プロンプト、ソースコード、Hook IPCトークンは、VINTAGEが保存・ログ出力しません。
- ターミナルで入力したコマンドは、そのシェルと同じローカル権限で実行されます。実行前に内容を確認してください。
- ファイルアクセスは登録済みワークスペースに制限し、プレビュー前にサイズを制限します。
- ペイン、タブ、ワークスペース、アプリケーションを閉じるとターミナルセッションを停止します。ワークスペースを削除しても実ディレクトリやファイルは削除しません。
- エージェントCLIや接続先サービスは、VINTAGEとは別にデータを保持する場合があります。各エージェントのポリシーを確認してください。

## 開発

必要な環境：

- Rust 1.95.0
- LinuxまたはWindowsの対応デスクトップ環境
- 手動確認に使う任意のエージェントCLI

```bash
git clone https://github.com/Tomatio13/vintage.git
cd vintage
cargo +1.95.0 run -p vintage-gpui --locked --
```

ネイティブチェック：

```bash
cargo +1.95.0 fmt --all --check
cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.95.0 test --workspace --locked
```

ローカルパッケージ作成：

```bash
tools/package-gpui-deb.sh
tools/package-gpui-appimage.sh
```

### 技術スタック

- Rust 2021とGPUI
- ランタイムが管理する擬似端末と`alacritty_terminal`
- ネイティブなワークスペース、ターミナル、設定、ファイル、Hook IPCサービス
- Linuxの`.deb`／AppImageとWindowsセットアップを作るGitHub Actions

## コントリビューション

コミュニティからの提案は[GitHub Issues](https://github.com/Tomatio13/vintage/issues/new/choose)で受け付けます。報告や提案の前に[CONTRIBUTING.md](CONTRIBUTING.md)を確認してください。

- Issueを作る前に既存Issueを検索してください。
- バグ、機能要望、ドキュメント改善に適したフォームを使ってください。
- 1つのIssueは1つの話題に絞り、報告や添付から機密情報を除いてください。
- メンテナーに実装を依頼されるまで、プルリクエストを作成しないでください。

## リリース

<details>
<summary>メンテナー向けリリース手順</summary>

1. `Cargo.toml`のワークスペースバージョンを更新し、対応するGitタグを作成します。
1. リリースコミットとタグをGitHubへpushします。
1. Actionsページで**Publish release assets**を実行し、`v0.5.1`のようなタグを入力します。
1. ワークフローは、そのReleaseを作成または更新して、Linux AppImage、Linux Debianパッケージ、WindowsセットアップEXEを添付します。
1. Releaseを公開する前に、対象OSで成果物をテストします。

ワークフローは次の資産を公開します。

- `VINTAGE_<version>_amd64.AppImage`
- `VINTAGE_<version>_amd64.deb`
- `VINTAGE_<version>_x64-setup.exe`

</details>

## ライセンス

VINTAGEは[MIT License](LICENSE)で公開しています。
