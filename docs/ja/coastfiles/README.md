# Coastfiles

Coastfile は、プロジェクトのルートに配置される TOML 設定ファイルです。これは、そのプロジェクト用の分離された開発環境を構築して実行するために Coast が知る必要のあるすべての情報、つまり、どのサービスを実行するか、どのポートを転送するか、データをどのように扱うか、シークレットをどのように管理するかを Coast に伝えます。

すべての Coast プロジェクトには、少なくとも 1 つの Coastfile が必要です。ファイル名は常に `Coastfile` です（大文字の C、拡張子なし）。異なるワークフロー向けのバリアントが必要な場合は、`Coastfile.light` や `Coastfile.snap` のような型付き Coastfile を作成し、[ベースを継承](INHERITANCE.md)します。

Coastfile が Coast の他の部分とどのように関係しているかをより深く理解するには、[Coasts](../concepts_and_terminology/COASTS.md) と [Builds](../concepts_and_terminology/BUILDS.md) を参照してください。

## Quickstart

可能な限り最小の Coastfile:

```toml
[coast]
name = "my-app"
```

これにより、`coast exec` で入れる DinD コンテナが得られます。ほとんどのプロジェクトでは、`compose` 参照または [bare services](SERVICES.md) のいずれかが必要になります。

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
web = 3000
api = 8080
```

あるいは、compose を使わずに bare services を使う場合:

```toml
[coast]
name = "my-app"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
restart = "on-failure"

[ports]
web = 3000
```

`coast build` を実行してから `coast run dev-1` を実行すれば、分離された環境が得られます。

## Example Coastfiles

### Simple bare-service project

compose ファイルなしの Next.js アプリです。Coast が Node をインストールし、`npm install` を実行し、開発サーバーを直接起動します。

```toml
[coast]
name = "my-crm"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --turbopack --port 3002 --hostname 0.0.0.0"
port = 3002
restart = "on-failure"

[ports]
web = 3002
```

### Full-stack compose project

共有データベース、シークレット、ボリューム戦略、カスタムセットアップを備えたマルチサービスプロジェクトです。

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
primary_port = "web"

[coast.setup]
packages = ["nodejs", "npm", "python3", "curl", "git", "bash", "ca-certificates", "wget"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]

[ports]
web = 3000
backend = 8080
postgres = 5432
redis = 6379

[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass" }

[shared_services.redis]
image = "redis:7"
ports = [6379]

[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"

[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"

[omit]
services = ["monitoring", "admin-panel", "nginx-proxy"]

[assign]
default = "none"
[assign.services]
backend = "hot"
web = "hot"
```

### Lightweight test variant (inheritance)

ベースの Coastfile を拡張しつつ、バックエンドテストの実行に必要なものだけに絞り込みます。ポートなし、共有サービスなし、分離データベースです。

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "backend", "postgres", "redis"]
shared_services = ["postgres", "redis"]

[omit]
services = ["redis", "backend", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[assign]
default = "none"
[assign.services]
backend-test = "rebuild"
```

### Snapshot-seeded variant

各 coast インスタンスは、ホスト上にある既存のデータベースボリュームのコピーから開始し、その後はそれぞれ独立して分岐します。

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis", "mongodb"]

[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "infra_redis_data"
service = "redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
snapshot_source = "infra_mongodb_data"
service = "mongodb"
mount = "/data/db"
```

## Conventions

- ファイル名は `Coastfile` でなければならず（大文字の C、拡張子なし）、プロジェクトルートに配置する必要があります。
- 型付きバリアントは `Coastfile.{type}` というパターンを使います。たとえば `Coastfile.light`、`Coastfile.snap` です。[Inheritance and Types](INHERITANCE.md) を参照してください。
- 予約名 `Coastfile.default` は使用できません。
- 全体を通して TOML 構文を使用します。すべてのセクションヘッダーは `[brackets]` を使用し、名前付きエントリは `[section.name]` を使用します（array-of-tables ではありません）。
- 同じ Coastfile 内で `compose` と `[services]` の両方を使うことはできません。どちらか一方を選んでください。
- 相対パス（`compose`、`root` など）は、Coastfile の親ディレクトリを基準に解決されます。

## Reference

| Page | Sections | What it covers |
|------|----------|----------------|
| [Project and Setup](PROJECT.md) | `[coast]`, `[coast.setup]` | 名前、compose パス、ランタイム、worktree ディレクトリ、コンテナセットアップ |
| [Worktree Directories](WORKTREE_DIR.md) | `worktree_dir`, `default_worktree_dir` | ローカルおよび外部 worktree ディレクトリ、チルダパス、Codex/Claude 統合 |
| [Ports](PORTS.md) | `[ports]`, `[egress]` | ポート転送、egress 宣言、プライマリポート |
| [Volumes](VOLUMES.md) | `[volumes.*]` | 分離、共有、スナップショットシードのボリューム戦略 |
| [Shared Services](SHARED_SERVICES.md) | `[shared_services.*]` | ホストレベルのデータベースおよびインフラサービス |
| [Secrets](SECRETS.md) | `[secrets.*]`, `[inject]` | シークレットの抽出、注入、ホスト環境/ファイル転送 |
| [Bare Services](SERVICES.md) | `[services.*]` | Docker Compose を使わずにプロセスを直接実行する方法 |
| [Agent Shell](AGENT_SHELL.md) | `[agent_shell]` | コンテナ化されたエージェント TUI ランタイム |
| [MCP Servers](MCP.md) | `[mcp.*]`, `[mcp_clients.*]` | 内部およびホストプロキシ型 MCP サーバー、クライアントコネクタ |
| [Assign](ASSIGN.md) | `[assign]` | サービスごとのブランチ切り替え動作 |
| [Inheritance and Types](INHERITANCE.md) | `extends`, `includes`, `[unset]`, `[omit]` | 型付き Coastfile、構成、およびオーバーライド |
