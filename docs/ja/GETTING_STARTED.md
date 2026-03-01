# Coasts を始める

まだであれば、まず以下のインストールと要件を満たしてください。その後、このガイドではプロジェクトで Coast を使う方法を説明します。

## Installing

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*`coast daemon install` を実行しない場合、毎回必ず `coast daemon start` でデーモンを手動で起動する責任があります。*

## Requirements

- macOS
- Docker Desktop
- Git を使用しているプロジェクト
- Node.js
- `socat` *(Homebrew の `depends_on` 依存として `curl -fsSL https://coasts.dev/install | sh` でインストールされます)*

```text
Linux note: We have not tested Coasts on Linux yet, but Linux support is planned.
You can try to run Coasts on Linux today, but we do not provide guarantees that it will work correctly.
```

## Setting Up Coasts in a Project

プロジェクトのルートに Coastfile を追加します。インストール時は worktree 上にいないことを確認してください。

```text
my-project/
├── Coastfile              <-- this is what Coast reads
├── docker-compose.yml
├── Dockerfile
├── src/
│   └── ...
└── ...
```

`Coastfile` は既存のローカル開発リソースを参照し、Coasts 固有の設定を追加します。完全なスキーマについては [Coastfiles documentation](coastfiles/README.md) を参照してください。

```toml
[coast]
name = "my-project"
compose = "./docker-compose.yml"

[ports]
web = 3000
db = 5432
```

Coastfile は軽量な TOML ファイルで、*通常は* 既存の `docker-compose.yml` を参照します（コンテナ化されていないローカル開発セットアップでも動作します）。また、プロジェクトを並列実行するために必要な変更（ポートマッピング、ボリューム戦略、シークレット）を記述します。プロジェクトのルートに配置してください。

プロジェクト用の Coastfile を作成する最速の方法は、コーディングエージェントに作ってもらうことです。

Coasts CLI には、あらゆる AI エージェントに Coastfile の完全なスキーマと CLI を教える組み込みプロンプトが同梱されています。ここで確認できます: [installation_prompt.txt](installation_prompt.txt)

エージェントに直接渡すか、[installation prompt](installation_prompt.txt) をコピーしてエージェントのチャットに貼り付けてください。

```bash-emphasis
# Claude Code
claude -p "$(coast installation-prompt)"

# Codex
codex "$(coast installation-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast installation-prompt)"
```

このプロンプトは、Coastfile の TOML 形式、ボリューム戦略、シークレット注入、および関連するすべての CLI コマンドをカバーします。エージェントはプロジェクトを分析し、Coastfile を生成します。

## Your First Coast

最初の Coast を起動する前に、実行中の開発環境を停止してください。Docker Compose を使っている場合は `docker-compose down` を実行します。ローカルの開発サーバーを動かしている場合は停止してください。Coasts は独自にポートを管理するため、すでに待ち受けているものがあると競合します。

Coastfile の準備ができたら:

```bash
coast build
coast run dev-1
```

インスタンスが実行中であることを確認します:

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -             ~/dev/my-project
```

サービスがどのポートで待ち受けているかを確認します:

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

各インスタンスにはそれぞれ動的ポートのセットが割り当てられるため、複数のインスタンスを並べて実行できます。インスタンスをプロジェクトのカノニカルポートに戻して紐づけるには、チェックアウトします:

```bash
coast checkout dev-1
```

これによりランタイムがチェックアウトされ、プロジェクトのカノニカルポート（例: `3000`、`5432`）がこの Coast インスタンスへルーティングされます。

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -         ✓   ~/dev/my-project
```

プロジェクトの Coastguard 観測性 UI を起動するには:

```bash
coast ui
```

## What's Next?

- Coasts とどのようにやり取りするかをホストエージェントに理解させるために、[skill for your host agent](SKILLS_FOR_HOST_AGENTS.md) を設定する
