# Next.js アプリケーション

このレシピは、Postgres と Redis をバックエンドに持ち、必要に応じてバックグラウンドワーカーや補助サービスを含む Next.js アプリケーション向けです。このスタックでは、Next.js を高速な HMR のために Turbopack とともに [bare service](../concepts_and_terminology/BARE_SERVICES.md) として実行し、一方で Postgres と Redis はホスト上で [shared services](../concepts_and_terminology/SHARED_SERVICES.md) として実行されるため、すべての Coast インスタンスが同じデータを共有します。

このパターンは、次のような場合に適しています。

- プロジェクトが開発環境で Turbopack を使った Next.js を使用している
- アプリケーションの背後にデータベースおよびキャッシュ層（Postgres、Redis）がある
- インスタンスごとのデータベースセットアップなしで、複数の Coast インスタンスを並行して実行したい
- レスポンス内にコールバック URL を埋め込む NextAuth のような認証ライブラリを使用している

## 完全な Coastfile

```toml
[coast]
name = "my-nextjs-app"
primary_port = "web"
private_paths = ["packages/web/.next"]
worktree_dir = [".worktrees", ".claude/worktrees"]

[coast.setup]
packages = ["nodejs", "npm", "make", "git", "bash"]
run = [
    "npm install -g corepack",
    "corepack enable",
]

# --- Bare services: Next.js and background worker ---

[services.web]
install = [
    "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)",
    "cd /workspace && test -f config.json || echo {} > config.json",
    "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn prisma migrate dev",
]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev"
port = 3000
restart = "on-failure"
cache = ["node_modules"]

[services.worker]
install = "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)"
command = "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev:worker"
restart = "on-failure"
cache = ["node_modules"]

# --- Shared services: Postgres and Redis on the host ---

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["myapp_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_DB = "postgres", POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "postgres" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["myapp_redis_data:/data"]

# --- Secrets: connection strings for bare services ---

[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"

[secrets.redis_url]
extractor = "command"
run = "echo redis://host.docker.internal:6379"
inject = "env:REDIS_URL"

# --- Ports ---

[ports]
web = 3000
postgres = 5432
redis = 6379

# --- Assign: branch-switch behavior ---

[assign]
default = "none"
exclude_paths = ["docs", ".github", "scripts"]

[assign.services]
web = "hot"
worker = "hot"

[assign.rebuild_triggers]
web = ["package.json", "yarn.lock"]
worker = ["package.json", "yarn.lock"]
```

## プロジェクトとセットアップ

```toml
[coast]
name = "my-nextjs-app"
primary_port = "web"
private_paths = ["packages/web/.next"]
worktree_dir = [".worktrees", ".claude/worktrees"]
```

**`private_paths`** は Next.js にとって非常に重要です。Turbopack は起動時に `.next/dev/lock` にロックファイルを作成します。`private_paths` がない場合、同じブランチ上の 2 つ目の Coast インスタンスはそのロックを検出し、起動を拒否します。これを設定すると、各インスタンスはインスタンスごとのオーバーレイマウントを通じて独立した `.next` ディレクトリを持つことができます。[Private Paths](../concepts_and_terminology/PRIVATE_PATHS.md) を参照してください。

**`worktree_dir`** は git worktree が存在するディレクトリを列挙します。複数のコーディングエージェント（Claude Code、Cursor、Codex）を使用している場合、それぞれが異なる場所に worktree を作成することがあります。これらをすべて列挙することで、どのツールが作成したかに関係なく、Coast が worktree を検出して割り当てられるようになります。

```toml
[coast.setup]
packages = ["nodejs", "npm", "make", "git", "bash"]
run = [
    "npm install -g corepack",
    "corepack enable",
]
```

セットアップセクションでは、bare service に必要なシステムパッケージとツールをインストールします。`corepack enable` は、プロジェクトの `packageManager` フィールドに基づいて yarn または pnpm を有効化します。これらはインスタンス起動時ではなく、Coast イメージ内でビルド時に実行されます。

## Bare Services

```toml
[services.web]
install = [
    "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)",
    "cd /workspace && test -f config.json || echo {} > config.json",
    "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn prisma migrate dev",
]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev"
port = 3000
restart = "on-failure"
cache = ["node_modules"]
```

**条件付きインストール:** `test -f node_modules/.yarn-state.yml || make yarn` というパターンは、`node_modules` がすでに存在する場合に依存関係のインストールをスキップします。これにより、依存関係が変更されていない場合のブランチ切り替えが高速になります。[Bare Service Optimization](../concepts_and_terminology/BARE_SERVICE_OPTIMIZATION.md) を参照してください。

**`cache`:** worktree の切り替えをまたいで `node_modules` を保持するため、`yarn install` は毎回最初からではなく増分的に実行されます。

**動的ポート付きの `AUTH_URL`:** NextAuth（または類似の認証ライブラリ）を使用する Next.js アプリケーションは、レスポンスにコールバック URL を埋め込みます。Coast の内部では Next.js はポート 3000 で待ち受けますが、ホスト側のポートは動的です。Coast はコンテナ環境に `WEB_DYNAMIC_PORT` を自動で注入します（`[ports]` の `web` キーから導出されます）。`:-3000` のフォールバックにより、同じコマンドを Coast の外でも使えます。[Dynamic Port Environment Variables](../concepts_and_terminology/DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) を参照してください。

**`host.docker.internal`:** bare service は `localhost` 経由では shared service に到達できません。shared service はホストの Docker デーモン上で動作しているためです。`host.docker.internal` は Coast コンテナの内部から見たホストを解決します。

## Shared Services

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["myapp_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_DB = "postgres", POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "postgres" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["myapp_redis_data:/data"]
```

Postgres と Redis は、ホストの Docker デーモン上で [shared services](../concepts_and_terminology/SHARED_SERVICES.md) として実行されます。すべての Coast インスタンスが同じデータベースに接続するため、ユーザー、セッション、データはインスタンス間で共有されます。これにより、各インスタンスごとに別々にサインアップする必要があるという問題を回避できます。

プロジェクトにすでに Postgres と Redis を含む `docker-compose.yml` がある場合は、代わりに `compose` を使用し、ボリューム戦略を `shared` に設定できます。bare-service の Coastfile では、管理すべき compose ファイルがないため、shared services の方がシンプルです。

## Secrets

```toml
[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"

[secrets.redis_url]
extractor = "command"
run = "echo redis://host.docker.internal:6379"
inject = "env:REDIS_URL"
```

これらはビルド時に `DATABASE_URL` と `REDIS_URL` を Coast コンテナ環境に注入します。接続文字列は `host.docker.internal` 経由で shared service を指します。

`command` extractor はシェルコマンドを実行し、その stdout を取得します。ここでは単に静的な文字列を echo しているだけですが、vault から読み取ったり、CLI ツールを実行したり、値を動的に計算したりする用途にも使えます。

bare service の `command` フィールドでも、これらの変数がインラインで設定されている点に注意してください。インラインの値が優先されますが、注入された secret は `install` ステップや `coast exec` セッションのデフォルトとして機能します。

## Assign 戦略

```toml
[assign]
default = "none"
exclude_paths = ["docs", ".github", "scripts"]

[assign.services]
web = "hot"
worker = "hot"

[assign.rebuild_triggers]
web = ["package.json", "yarn.lock"]
worker = ["package.json", "yarn.lock"]
```

**`default = "none"`** は、ブランチ切り替え時に shared service とインフラストラクチャをそのままにします。コードに依存するサービスだけが assign 戦略を持ちます。

**Next.js とワーカーに対する `hot`:** Turbopack を使う Next.js には組み込みのホットモジュールリプレースメントがあります。Coast が `/workspace` を新しい worktree に再マウントすると、Turbopack がファイルの変更を検出して自動的に再コンパイルします。プロセスの再起動は不要です。`tsc --watch` や `nodemon` を使うバックグラウンドワーカーも、ファイルウォッチャーを通じて変更を取り込みます。

**`rebuild_triggers`:** ブランチ間で `package.json` または `yarn.lock` が変更されていた場合、サービスの `install` コマンドはサービス再起動前に再実行されます。これにより、パッケージの追加または削除を伴うブランチ切り替え後でも依存関係が最新の状態になります。

**`exclude_paths`:** サービスが必要としないディレクトリをスキップすることで、初回の worktree ブートストラップを高速化します。ドキュメント、CI 設定、スクリプトは除外しても安全です。

## このレシピの調整

**バックグラウンドワーカーがない場合:** `[services.worker]` セクションとその assign エントリを削除してください。Coastfile の残りは変更なしで動作します。

**複数の Next.js アプリを含むモノレポ:** 各アプリの `.next` ディレクトリに対して `private_paths` エントリを追加してください。各 bare service は適切な `command` と `port` を持つ独自の `[services.*]` セクションを持ちます。

**yarn ではなく pnpm の場合:** `make yarn` を pnpm のインストールコマンドに置き換えてください。pnpm が依存関係を別の場所（例: `.pnpm-store`）に保存する場合は、`cache` フィールドも調整してください。

**shared services を使わない場合:** インスタンスごとのデータベースを好む場合は、`[shared_services]` と `[secrets]` セクションを削除してください。Postgres と Redis を `docker-compose.yml` に追加し、`[coast]` セクションで `compose` を設定し、分離を制御するために [volume strategies](../coastfiles/VOLUMES.md) を使用してください。インスタンスごとのデータには `strategy = "isolated"` を、共有データには `strategy = "shared"` を使います。

**追加の認証プロバイダー:** 認証ライブラリがコールバック URL に `AUTH_URL` 以外の環境変数を使用する場合は、サービスコマンド内のそれらの変数にも同じ `${WEB_DYNAMIC_PORT:-3000}` パターンを適用してください。
