# Agent Shells

エージェントシェルは、Coast の内部にあるシェルで、エージェントの TUI ランタイム（Claude Code、Codex、または任意の CLI エージェント）に直接接続して開きます。Coastfile の `[agent_shell]` セクションで設定し、Coast が DinD コンテナ内でエージェントプロセスを起動します。

**ほとんどのユースケースでは、これを行うべきではありません。** 代わりにホストマシン上でコーディングエージェントを実行してください。共有[ファイルシステム](FILESYSTEM.md)により、ホスト側のエージェントは通常どおりコードを編集しながら、ランタイム情報のために [`coast logs`](LOGS.md)、[`coast exec`](EXEC_AND_DOCKER.md)、[`coast ps`](RUNTIMES_AND_SERVICES.md) を呼び出せます。エージェントシェルは、資格情報のマウント、OAuth の複雑さ、ライフサイクルの複雑さを追加します。エージェント自体をコンテナ化する明確な理由がない限り、これらは不要です。

## The OAuth Problem

Claude Code、Codex、または同様の OAuth で認証するツールを使用している場合、トークンはホストマシン向けに発行されています。その同じトークンを Linux コンテナ内（異なるユーザーエージェント、異なる環境）から使用すると、プロバイダがそれを検知してフラグ付けしたり失効させたりする可能性があります。デバッグが難しい断続的な認証失敗が発生します。

コンテナ化されたエージェントでは、API キーベースの認証の方が安全な選択です。キーを Coastfile の [secret](SECRETS.md) として設定し、コンテナ環境へ注入してください。

API キーが使えない場合は、OAuth 資格情報を Coast にマウントできます（下の Configuration セクション参照）が、摩擦があることを想定してください。macOS で `keychain` シークレット抽出器を使って OAuth トークンを取り出す場合、`coast build` のたびに macOS キーチェーンのパスワード入力を求められます。これは特に頻繁に再ビルドする場合に面倒です。キーチェーンのプロンプトは macOS のセキュリティ要件であり、回避できません。

## Configuration

Coastfile に `[agent_shell]` セクションを追加し、実行するコマンドを指定します:

```toml
[agent_shell]
command = "claude --dangerously-skip-permissions"
```

このコマンドは DinD コンテナ内の `/workspace` で実行されます。Coast はコンテナ内に `coast` ユーザーを作成し、資格情報を `/root/.claude/` から `/home/coast/.claude/` にコピーして、そのユーザーとしてコマンドを実行します。エージェントがコンテナに資格情報をマウントする必要がある場合は、ファイル注入付きの `[secrets]`（[Secrets and Extractors](SECRETS.md) を参照）と、エージェント CLI をインストールするための `[coast.setup]` を使用してください:

```toml
[coast.setup]
run = ["npm install -g @anthropic-ai/claude-code"]

[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[agent_shell]
command = "claude --dangerously-skip-permissions"
```

`[agent_shell]` が設定されている場合、インスタンス起動時に Coast は自動でシェルを起動します。設定は `extends` により継承され、[Coastfile type](COASTFILE_TYPES.md) ごとに上書きできます。

## The Active Agent Model

各 Coast インスタンスは複数のエージェントシェルを持てますが、同時に **active** になれるのは 1 つだけです。active シェルは、`--shell` ID を指定しないコマンドのデフォルトターゲットになります。

```bash
coast agent-shell dev-1 ls

  SHELL  STATUS   ACTIVE
  1      running  ★
  2      running
```

active シェルを切り替えます:

```bash
coast agent-shell dev-1 activate 2
```

active シェルは閉じられません — 先に別のものを active にしてください。これは、操作中のシェルを誤って終了させるのを防ぎます。

Coastguard では、エージェントシェルは Exec パネル内のタブとして表示され、active/inactive のバッジが付きます。タブをクリックして端末を表示し、ドロップダウンメニューから有効化、起動、または閉じる操作ができます。

![Agent shell in Coastguard](../../assets/coastguard-agent-shell.png)
*Coastguard の Exec タブからアクセスできる、Coast インスタンス内で Claude Code を実行しているエージェントシェル。*

## Sending Input

コンテナ化されたエージェントをプログラム的に駆動する主な方法は `coast agent-shell input` です:

```bash
coast agent-shell dev-1 input "fix the failing test in auth.test.ts"
```

これはテキストを active エージェントの TUI に書き込み、Enter を押します。エージェントは、端末に手入力したかのように受け取ります。

オプション:

- `--no-send` — Enter を押さずにテキストを書き込みます。部分的な入力を組み立てたり、TUI メニューを操作したりするのに便利です。
- `--shell <id>` — active ではなく特定のシェルを対象にします。
- `--show-bytes` — デバッグのため、送信される正確なバイト列を表示します。

内部的には、入力は PTY のマスターファイルディスクリプタに直接書き込まれます。テキストと Enter キーストロークは 2 回の別々の write として送られ、その間に 25ms の間隔を置きます。これは、一部の TUI フレームワークが高速入力を受け取った際に示すペーストモードの副作用を避けるためです。

## Other Commands

```bash
coast agent-shell dev-1 spawn              # create a new shell
coast agent-shell dev-1 spawn --activate   # create and immediately activate
coast agent-shell dev-1 tty                # attach interactive TTY to active shell
coast agent-shell dev-1 tty --shell 2      # attach to a specific shell
coast agent-shell dev-1 read-output        # read full scrollback buffer
coast agent-shell dev-1 read-last-lines 50 # read last 50 lines of output
coast agent-shell dev-1 session-status     # check if the shell process is alive
```

`tty` はライブのインタラクティブセッションを提供します — エージェントの TUI に直接入力できます。標準の端末エスケープシーケンスでデタッチできます。`read-output` と `read-last-lines` は非インタラクティブでテキストを返すため、スクリプトや自動化に便利です。

## Lifecycle and Recovery

エージェントシェルのセッションは、Coastguard でページ移動しても維持されます。スクロールバックバッファ（最大 512KB）は、タブへ再接続した際にリプレイされます。

`coast stop` で Coast インスタンスを停止すると、すべてのエージェントシェル PTY プロセスが kill され、データベースレコードがクリーンアップされます。`[agent_shell]` が設定されている場合、`coast start` は新しいエージェントシェルを自動起動します。

デーモンの再起動後、以前動作していたエージェントシェルは dead と表示されます。システムはこれを自動検出します — active シェルが dead の場合、最初の live シェルが active に昇格します。生きているシェルが 1 つもない場合は、`coast agent-shell spawn --activate` で新しいものを起動してください。

## Who This Is For

エージェントシェルは、Coasts の周りに **ファーストパーティ統合を構築するプロダクト** 向けに設計されています — オーケストレーションプラットフォーム、エージェントラッパー、そして `input`、`read-output`、`session-status` API を通じてコンテナ化されたコーディングエージェントをプログラム的に管理したいツールです。

一般用途の並列エージェントコーディングでは、ホスト上でエージェントを実行してください。よりシンプルで、OAuth 問題を回避し、資格情報マウントの複雑さを避け、共有ファイルシステムの利点を最大限に活用できます。エージェントコンテナ化のオーバーヘッドなしに、Coast の利点（分離されたランタイム、ポート管理、worktree 切り替え）をすべて得られます。

エージェントシェルのさらに上の複雑さの段階は、コンテナ化されたエージェントがツールへアクセスできるように [MCP servers](MCP_SERVERS.md) を Coast にマウントすることです。これは統合面をさらに拡大し、別途取り上げています。必要ならその機能はありますが、ほとんどのユーザーには不要です。
