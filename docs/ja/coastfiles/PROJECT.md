# プロジェクトとセットアップ

`[coast]` セクションは Coastfile で唯一必須のセクションです。プロジェクトを識別し、Coast コンテナがどのように作成されるかを設定します。任意の `[coast.setup]` サブセクションでは、ビルド時にコンテナ内でパッケージをインストールしたりコマンドを実行したりできます。

## `[coast]`

### `name`（必須）

プロジェクトの一意な識別子です。コンテナ名、ボリューム名、状態管理、CLI 出力で使用されます。

```toml
[coast]
name = "my-app"
```

### `compose`

Docker Compose ファイルへのパスです。相対パスはプロジェクトルート（Coastfile を含むディレクトリ、または `root` が設定されている場合はそのディレクトリ）を基準に解決されます。

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
```

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
```

省略した場合、Coast コンテナは `docker compose up` を実行せずに起動します。[bare services](SERVICES.md) を使用するか、`coast exec` を介してコンテナを直接操作できます。

同じ Coastfile 内で `compose` と `[services]` の両方を設定することはできません。

### `runtime`

使用するコンテナランタイムです。デフォルトは `"dind"`（Docker-in-Docker）です。

- `"dind"` — `--privileged` 付きの Docker-in-Docker。プロダクションで唯一テストされているランタイムです。[Runtimes and Services](../concepts_and_terminology/RUNTIMES_AND_SERVICES.md) を参照してください。
- `"sysbox"` — 特権モードの代わりに Sysbox ランタイムを使用します。Sysbox のインストールが必要です。
- `"podman"` — 内部のコンテナランタイムとして Podman を使用します。

```toml
[coast]
name = "my-app"
runtime = "dind"
```

### `root`

プロジェクトルートディレクトリを上書きします。デフォルトでは、プロジェクトルートは Coastfile を含むディレクトリです。相対パスは Coastfile のディレクトリを基準に解決され、絶対パスはそのまま使用されます。

```toml
[coast]
name = "my-app"
root = "../my-project"
```

これは一般的ではありません。ほとんどのプロジェクトでは、Coastfile を実際のプロジェクトルートに置きます。

### `worktree_dir`

git worktree が存在するディレクトリです。単一の文字列または文字列の配列を受け付けます。デフォルトは `".worktrees"` です。

```toml
# Single directory
worktree_dir = ".worktrees"

# Multiple directories, including an external one
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

相対パスはプロジェクトルートを基準に解決されます。`~/` または `/` で始まるパスは**外部**ディレクトリとして扱われ、Coast はコンテナからアクセスできるように別個のバインドマウントを追加します。これは、プロジェクトルートの外側に worktree を作成する Codex のようなツールと統合する方法です。

実行時に Coast は既存の git worktree（`git worktree list` 経由）から worktree ディレクトリを自動検出し、すべての worktree が単一のディレクトリで一致している場合は、設定されたデフォルトよりもそれを優先します。

外部ディレクトリの挙動、プロジェクトフィルタリング、例を含む完全なリファレンスについては、[Worktree Directories](WORKTREE_DIR.md) を参照してください。

### `default_worktree_dir`

**新しい** worktree を作成するときに使用するディレクトリです。デフォルトは `worktree_dir` の最初のエントリです。`worktree_dir` が配列である場合にのみ関係します。

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
default_worktree_dir = ".worktrees"
```

### `autostart`

`coast run` で Coast インスタンスが作成されたときに、`docker compose up`（または bare services の起動）を自動実行するかどうかです。デフォルトは `true` です。

コンテナは起動しておきつつサービスは手動で起動したい場合は `false` にします。必要に応じてテストを実行するテストランナーのバリアントに便利です。

```toml
[coast]
name = "my-app"
extends = "Coastfile"
autostart = false
```

### `primary_port`

クイックリンクおよびサブドメインルーティングに使用するために、`[ports]` セクションのポート名を指定します。値は `[ports]` で定義されたキーと一致している必要があります。

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

これによりサブドメインルーティングと URL テンプレートがどのように有効になるかは、[Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md) を参照してください。

## `[coast.setup]`

Coast コンテナ自体をカスタマイズします。ツールのインストール、ビルド手順の実行、設定ファイルの生成などを行います。`[coast.setup]` 内のすべては DinD コンテナ内で実行されます（compose サービス内ではありません）。

### `packages`

インストールする APK パッケージです。ベースの DinD イメージは Alpine ベースのため、これらは Alpine Linux のパッケージです。

```toml
[coast.setup]
packages = ["nodejs", "npm", "git", "curl"]
```

### `run`

ビルド中に順番に実行されるシェルコマンドです。APK パッケージとして利用できないツールのインストールに使用します。

```toml
[coast.setup]
packages = ["nodejs", "npm", "python3", "wget", "bash", "ca-certificates"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]
```

### `[[coast.setup.files]]`

コンテナ内に作成するファイルです。各エントリには `path`（必須、絶対パスである必要があります）、`content`（必須）、および任意の `mode`（3〜4 桁の 8 進文字列）があります。

```toml
[coast.setup]
packages = ["nodejs", "npm"]
run = ["mkdir -p /app/config"]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```

ファイルエントリの検証ルール:

- `path` は絶対パス（`/` で始まる）でなければならない
- `path` は `..` コンポーネントを含んではならない
- `path` は `/` で終わってはならない
- `mode` は 3 桁または 4 桁の 8 進文字列でなければならない（例: `"600"`、`"0644"`）

## 完全な例

Go と Node.js 開発向けにセットアップされた Coast コンテナ:

```toml
[coast]
name = "my-fullstack-app"
compose = "./docker-compose.yml"
runtime = "dind"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
primary_port = "web"

[coast.setup]
packages = ["nodejs", "npm", "python3", "make", "curl", "git", "bash", "ca-certificates", "wget", "gcc", "musl-dev"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz && ln -s /usr/local/go/bin/go /usr/local/bin/go",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
    "pip3 install --break-system-packages pgcli",
]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```
