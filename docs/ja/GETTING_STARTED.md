# Coasts 入門

まだの場合は、まず以下のインストールと要件を完了してください。その後、このガイドでプロジェクトで Coast を使う手順を説明します。

## インストール

```bash
curl -fsSL https://coasts.dev/install | sh
coast daemon install
```

*`coast daemon install` を実行しないと決めた場合、毎回 `coast daemon start` でデーモンを手動で起動する責任はあなたにあります。*

## 要件

- macOS
- Docker Desktop
- Git を使用しているプロジェクト
- Node.js
- `socat`（macOS では `brew install socat`）

```text
Linux note: We have not tested Coasts on Linux yet, but Linux support is planned.
You can try to run Coasts on Linux today, but we do not provide guarantees that it will work correctly.
```

## プロジェクトで Coasts をセットアップする

プロジェクトのルートに Coastfile を追加します。インストール時は worktree 上にいないことを確認してください。

```text
my-project/
├── Coastfile              <-- これを Coast が読み込みます
├── docker-compose.yml
├── Dockerfile
├── src/
│   └── ...
└── ...
```

`Coastfile` は既存のローカル開発リソースを指し、Coasts 固有の設定を追加します。完全なスキーマは [Coastfiles documentation](coastfiles/README.md) を参照してください:

```toml
[coast]
name = "my-project"
compose = "./docker-compose.yml"

[ports]
web = 3000
db = 5432
```

Coastfile は軽量な TOML ファイルで、*通常* は既存の `docker-compose.yml` を指します（コンテナ化されていないローカル開発セットアップでも動作します）。また、プロジェクトを並列で動かすために必要な変更点（ポートマッピング、ボリューム戦略、シークレット）を記述します。プロジェクトのルートに配置してください。

プロジェクト用の Coastfile を作成する最速の方法は、コーディングエージェントに作ってもらうことです。

Coasts CLI には、任意の AI エージェントに Coastfile の完全なスキーマと CLI を教えるための組み込みプロンプトが付属しています。ここで確認できます: [installation_prompt.txt](installation_prompt.txt)

エージェントに直接渡すか、[installation prompt](installation_prompt.txt) をコピーしてエージェントのチャットに貼り付けてください:

```bash-emphasis
# Claude Code
claude -p "$(coast installation-prompt)"

# Codex
codex "$(coast installation-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast installation-prompt)"
```

このプロンプトは、Coastfile の TOML 形式、ボリューム戦略、シークレット注入、関連するすべての CLI コマンドをカバーしています。エージェントがプロジェクトを解析し、Coastfile を生成します。

## 最初の Coast

最初の Coast を起動する前に、実行中の開発環境をすべて停止してください。Docker Compose を使っている場合は `docker-compose down` を実行します。ローカルの開発サーバーを動かしている場合は停止してください。Coasts は自身でポートを管理するため、すでに待ち受けているものがあると競合します。

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

サービスがどのポートで待ち受けているか確認します:

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

各インスタンスには専用の動的ポート一式が割り当てられるため、複数のインスタンスを並べて同時に実行できます。インスタンスをプロジェクトの正規（canonical）ポートに紐づけるには、チェックアウトします:

```bash
coast checkout dev-1
```

これは、ランタイムがチェックアウトされ、プロジェクトの正規ポート（`3000`、`5432` など）がこの Coast インスタンスへルーティングされるようになったことを意味します。

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -         ✓   ~/dev/my-project
```

プロジェクト向けに Coastguard の可観測性 UI を起動するには:

```bash
coast ui
```

## 次は？

- Coasts とやり取りする方法を理解させるために、[ホストエージェント用のスキル](SKILLS_FOR_HOST_AGENTS.md) を設定する
