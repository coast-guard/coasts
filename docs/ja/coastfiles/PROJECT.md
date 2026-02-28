# プロジェクトとセットアップ

Coastfile で必須なのは `[coast]` セクションだけです。これはプロジェクトを識別し、Coast コンテナがどのように作成されるかを設定します。オプションの `[coast.setup]` サブセクションでは、ビルド時にコンテナ内でパッケージをインストールしたりコマンドを実行したりできます。

## `[coast]`

### `name`（必須）

プロジェクトの一意な識別子。コンテナ名、ボリューム名、状態追跡、CLI 出力で使用されます。

```toml
[coast]
name = "my-app"
```

### `compose`

Docker Compose ファイルへのパス。相対パスはプロジェクトルート（Coastfile を含むディレクトリ、または `root` が設定されている場合は `root`）を基準に解決されます。

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

省略した場合、Coast コンテナは `docker compose up` を実行せずに起動します。[ベアサービス](SERVICES.md)を使うか、`coast exec` でコンテナに直接アクセスして操作できます。

同じ Coastfile で `compose` と `[services]` の両方を設定することはできません。

### `runtime`

使用するコンテナランタイム。デフォルトは `"dind"`（Docker-in-Docker）です。

- `"dind"` — `--privileged` を使う Docker-in-Docker。唯一プロダクションで検証されたランタイム。[Runtimes and Services](../concepts_and_terminology/RUNTIMES_AND_SERVICES.md) を参照してください。
- `"sysbox"` — 特権モードの代わりに Sysbox ランタイムを使用します。Sysbox のインストールが必要です。
- `"podman"` — 内側のコンテナランタイムとして Podman を使用します。

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

Coast インスタンス用に git worktree が作成されるディレクトリ。デフォルトは `".coasts"`。相対パスはプロジェクトルートを基準に解決されます。

```toml
[coast]
name = "my-app"
worktree_dir = ".worktrees"
```

ディレクトリが相対パスで、かつプロジェクト内にある場合、Coast は自動的にそれを `.gitignore` に追加します。

### `autostart`

`coast run` で Coast インスタンスが作成されたときに、`docker compose up`（またはベアサービスの起動）を自動で実行するかどうか。デフォルトは `true` です。

コンテナは起動していてほしいがサービスは手動で起動したい場合は `false` に設定します。必要に応じてテストを実行するテストランナーのバリアントなどに便利です。

```toml
[coast]
name = "my-app"
extends = "Coastfile"
autostart = false
```

### `primary_port`

クイックリンクおよびサブドメインルーティングに使用するため、`[ports]` セクションからポート名を指定します。値は `[ports]` で定義されたキーと一致している必要があります。

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

これによりサブドメインルーティングと URL テンプレートが有効になる仕組みは、[Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md) を参照してください。

## `[coast.setup]`

Coast コンテナ自体をカスタマイズします — ツールのインストール、ビルド手順の実行、設定ファイルの生成など。`[coast.setup]` 内の内容はすべて DinD コンテナ内で実行されます（compose のサービス内ではありません）。

### `packages`

インストールする APK パッケージ。ベースの DinD イメージが Alpine ベースのため、Alpine Linux のパッケージになります。

```toml
[coast.setup]
packages = ["nodejs", "npm", "git", "curl"]
```

### `run`

ビルド中に順番に実行されるシェルコマンド。APK パッケージとして提供されていないツールのインストールに使用します。

```toml
[coast.setup]
packages = ["nodejs", "npm", "python3", "wget", "bash", "ca-certificates"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]
```

### `[[coast.setup.files]]`

コンテナ内に作成するファイル。各エントリには `path`（必須、絶対パスである必要あり）、`content`（必須）、および任意の `mode`（3〜4 桁の 8 進数文字列）があります。

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

- `path` は絶対パスである必要があります（`/` で始まる）
- `path` に `..` コンポーネントを含めてはいけません
- `path` は `/` で終わってはいけません
- `mode` は 3 桁または 4 桁の 8 進数文字列である必要があります（例: `"600"`, `"0644"`）

## 完全な例

Go と Node.js 開発向けにセットアップされた Coast コンテナ:

```toml
[coast]
name = "my-fullstack-app"
compose = "./docker-compose.yml"
runtime = "dind"
worktree_dir = ".worktrees"
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
