# フルスタック・モノレポ

このレシピは、共有データベースとキャッシュ層を背後に持つ複数のWebアプリケーションを含む大規模なモノレポ向けです。このスタックは、重量級のバックエンドサービス（Rails、Sidekiq、SSR）に Docker Compose を使用し、Vite の開発サーバーは DinD ホスト上でベアサービスとして実行します。Postgres と Redis はホストの Docker デーモン上で共有サービスとして動作するため、すべての Coast インスタンスが同一のインフラに接続し、重複して起動することはありません。

このパターンは次のような場合にうまく機能します:

- モノレポにデータベースを共有する複数のアプリが含まれている
- 各 Coast インスタンスでそれぞれ Postgres と Redis を起動しない軽量な Coast インスタンスが欲しい
- フロントエンドの開発サーバーに、compose コンテナの内部から `host.docker.internal` 経由で到達できる必要がある
- `localhost:5432` に接続するホスト側の MCP 連携があり、それらを変更せずに動かし続けたい

## 完全な Coastfile

以下が完全な Coastfile です。各セクションは後ほど詳しく説明します。

```toml
[coast]
name = "acme"
compose = "./infra/docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "bash"]
run = ["npm install -g yarn"]

[ports]
api = 3000
web = 3002
vite-web = 3040
vite-api = 3036

[healthcheck]
web = "/"
api = "/"

# --- Bare services: Vite dev servers on the DinD host ---

[services.vite-web]
install = "cd /workspace && yarn install --immutable 2>/dev/null || yarn install"
command = "cd /workspace && yarn workspace @acme/web run dev"
port = 3040
restart = "on-failure"
cache = ["node_modules"]

[services.vite-api]
command = "cd /workspace && yarn workspace @acme/api run dev"
port = 3036
restart = "on-failure"

# --- Shared services: Postgres and Redis on the host daemon ---

[shared_services.db]
image = "postgres:15.3-alpine"
ports = [5432]
volumes = ["infra_postgres:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "password" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis:/data"]

# --- Volumes: shared caches across all instances ---

[volumes.bundle]
strategy = "shared"
service = "api-rails"
mount = "/usr/local/bundle"

[volumes.api_rails_cache]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/tmp/cache"

[volumes.api_assets]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/public/assets"

[volumes.web_rails_cache]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/tmp/cache"

[volumes.web_assets]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/public/assets"

# --- Secrets and injection ---

[secrets.compose_profiles]
extractor = "command"
run = "echo api,web"
inject = "env:COMPOSE_PROFILES"

[secrets.uid]
extractor = "command"
run = "id -u"
inject = "env:UID"

[secrets.gid]
extractor = "command"
run = "id -g"
inject = "env:GID"

[inject]
env = ["USER", "BUNDLE_GEMS__CONTRIBSYS__COM"]

# --- Assign: branch-switch behavior ---

[assign]
default = "none"
exclude_paths = [
    ".coasts",
    ".yarn",
    ".github",
    "docs",
    "scripts",
    "cli",
    "deploy",
    "tools",
    "mobile",
    "extensions",
]

[assign.services]
web-rails = "hot"
web-ssr = "hot"
web-sidekiq = "restart"
api-rails = "hot"
api-sidekiq = "restart"

[assign.rebuild_triggers]
web-rails = ["infra/Dockerfile", "web/Gemfile", "web/Gemfile.lock", "web/package.json"]
api-rails = ["infra/Dockerfile", "api/Gemfile", "api/Gemfile.lock", "api/package.json"]
```

## プロジェクトと Compose

```toml
[coast]
name = "acme"
compose = "./infra/docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "bash"]
run = ["npm install -g yarn"]
```

`compose` フィールドは既存の Docker Compose ファイルを指します。Coast は `coast run` 時に DinD コンテナ内で `docker compose up -d` を実行するため、バックエンドサービス（Rails サーバー、Sidekiq ワーカー、SSR プロセス）が自動的に起動します。

`[coast.setup]` は DinD ホスト自身にパッケージをインストールします — compose コンテナの内部ではありません。これはホスト上で直接動くベアサービス（Vite 開発サーバー）に必要です。compose サービスは通常どおり Dockerfile からランタイムを取得します。

## 共有サービス

```toml
[shared_services.db]
image = "postgres:15.3-alpine"
ports = [5432]
volumes = ["infra_postgres:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "password" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis:/data"]
```

Postgres と Redis は、各 Coast の中で動かすのではなく [共有サービス](../concepts_and_terminology/SHARED_SERVICES.md) として宣言します。つまり、ホストの Docker デーモン上で動作し、すべての Coast インスタンスがブリッジネットワーク越しにそれらへ接続します。

**なぜ compose 内部の DB ではなく共有サービスなのか？**

- **インスタンスが軽い。** 各 Coast で Postgres と Redis のコンテナを個別に起動しないため、メモリと起動時間を節約できます。
- **ホストボリュームの再利用。** `volumes` フィールドは既存の Docker ボリューム（ローカルで `docker-compose up` により作成されたもの）を参照します。すでにあるデータが即座に利用でき、シーディングやマイグレーションの再実行が不要です。
- **MCP 互換性。** ホスト上で `localhost:5432` に接続しているデータベース MCP ツールがある場合、共有 Postgres は同じポートでホスト上にあるため、そのまま動作します。再設定は不要です。

**トレードオフ:** Coast インスタンス間でデータ分離がありません。すべてのインスタンスが同じデータベースを読み書きします。インスタンスごとの DB が必要なワークフローなら、代わりに `strategy = "isolated"` の [ボリューム戦略](../concepts_and_terminology/VOLUMES.md) を使うか、共有サービスに `auto_create_db = true` を設定して共有 Postgres 内にインスタンスごとの DB を作成してください。詳細は [Shared Services Coastfile reference](../coastfiles/SHARED_SERVICES.md) を参照してください。

**ボリューム名が重要です。** ボリューム名（`infra_postgres`、`infra_redis`）は、ローカルで `docker-compose up` を実行した際にホスト上にすでに存在するボリューム名と一致している必要があります。一致しない場合、共有サービスは空のボリュームで起動します。このセクションを書く前に `docker volume ls` を実行して既存のボリューム名を確認してください。

## ベアサービス

```toml
[services.vite-web]
install = "cd /workspace && yarn install --immutable 2>/dev/null || yarn install"
command = "cd /workspace && yarn workspace @acme/web run dev"
port = 3040
restart = "on-failure"
cache = ["node_modules"]

[services.vite-api]
command = "cd /workspace && yarn workspace @acme/api run dev"
port = 3036
restart = "on-failure"
```

Vite 開発サーバーは [ベアサービス](../concepts_and_terminology/BARE_SERVICES.md) として定義します — Docker Compose の外側で、DinD ホスト上で直接動く通常プロセスです。これは [混在サービス種別](../concepts_and_terminology/MIXED_SERVICE_TYPES.md) のパターンです。

**なぜ compose ではなくベアなのか？**

主な理由はネットワーキングです。Vite 開発サーバーへ到達する必要がある compose サービス（SSR、アセットのプロキシ、HMR の WebSocket 接続など）は、`host.docker.internal` を使って DinD ホスト上のベアサービスへ到達できます。これにより複雑な Docker ネットワーク設定を避けられ、多くのモノレポ構成で `VITE_RUBY_HOST` などの環境変数を設定する方法とも一致します。

また、ベアサービスはバインドマウントされた `/workspace` のファイルシステムへ、内側のコンテナのオーバーレイを経由せずに直接アクセスできます。そのため Vite のファイルウォッチャーが変更により速く反応します。

**`install` と `cache`:** `install` フィールドはサービス開始前、そして各 `coast assign` のたびに実行されます。ここでは `yarn install` を実行して、ブランチ切り替え時に依存関係の変更を取り込みます。`cache` フィールドは、worktree 切り替えを跨いで `node_modules` を保持するよう Coast に指示し、インストールを毎回ゼロからではなく増分にします。

**`install` は1つだけ:** `vite-api` には `install` フィールドがないことに注目してください。yarn workspaces のモノレポでは、ルートで一度 `yarn install` すれば全ワークスペースの依存関係が入ります。どれか一つのサービスにだけ置くことで二重実行を避けられます。

## ポートとヘルスチェック

```toml
[ports]
api = 3000
web = 3002
vite-web = 3040
vite-api = 3036

[healthcheck]
web = "/"
api = "/"
```

Coast に管理させたいポートはすべて `[ports]` に入れます。各インスタンスは、宣言した各ポートごとに [動的ポート](../concepts_and_terminology/PORTS.md)（高いレンジで、常にアクセス可能）を割り当てられます。[チェックアウト済み](../concepts_and_terminology/CHECKOUT.md) のインスタンスは、さらにホストへ正規ポート（宣言した番号）もフォワードされます。

`[healthcheck]` セクションは、各ポートの健全性をどのようにプローブするかを Coast に伝えます。ヘルスチェックパスが設定されたポートでは、Coast は 5 秒ごとに HTTP GET を送ります — どの HTTP 応答でも healthy とみなされます。ヘルスチェックパスがないポートは TCP 接続チェック（ポートが接続を受け付けられるか？）にフォールバックします。

この例では、Rails の web サーバーは HTML ページを返すため `/` に対して HTTP ヘルスチェックを行います。Vite 開発サーバーはヘルスチェックパスを設定していません — 意味のあるルートページを提供せず、接続を受け付けていることが分かれば十分なので TCP チェックで足ります。

ヘルスチェック状態は [Coastguard](../concepts_and_terminology/COASTGUARD.md) UI と `coast ports` で確認できます。

## ボリューム

```toml
[volumes.bundle]
strategy = "shared"
service = "api-rails"
mount = "/usr/local/bundle"

[volumes.api_rails_cache]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/tmp/cache"

[volumes.api_assets]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/public/assets"

[volumes.web_rails_cache]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/tmp/cache"

[volumes.web_assets]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/public/assets"
```

ここにあるボリュームはすべて `strategy = "shared"` を使用しています。これは単一の Docker ボリュームがすべての Coast インスタンスで共有されることを意味します。これは **キャッシュやビルド成果物** に対して正しい選択です — 同時書き込みが安全で、インスタンスごとに複製するとディスク容量を無駄にし、起動が遅くなる種類のものです:

- **`bundle`** — Ruby gem のキャッシュ。gem はブランチ間で同じです。共有することで各 Coast インスタンスごとに bundle 全体を再ダウンロードするのを避けられます。
- **`*_rails_cache`** — Rails のファイルベースキャッシュ。開発を高速化しますが重要データではなく、どのインスタンスでも再生成できます。
- **`*_assets`** — コンパイル済みアセット。キャッシュと同じ理由です。

**なぜ DB に shared を使わないのか？** Coast は、データベース的なサービスに接続されたボリュームで `strategy = "shared"` を使うと警告を表示します。複数の Postgres プロセスが同じデータディレクトリに書き込むと破損します。DB については、[共有サービス](../coastfiles/SHARED_SERVICES.md)（このレシピのようにホスト上で Postgres を1つ動かす）を使うか、`strategy = "isolated"`（各 Coast が専用ボリュームを持つ）を使ってください。完全な判断表は [Volume Topology](../concepts_and_terminology/VOLUMES.md) を参照してください。

## Assign 戦略

```toml
[assign]
default = "none"
exclude_paths = [
    ".coasts",
    ".yarn",
    ".github",
    "docs",
    "scripts",
    "cli",
    "deploy",
    "tools",
    "mobile",
    "extensions",
]

[assign.services]
web-rails = "hot"
web-ssr = "hot"
web-sidekiq = "restart"
api-rails = "hot"
api-sidekiq = "restart"

[assign.rebuild_triggers]
web-rails = ["infra/Dockerfile", "web/Gemfile", "web/Gemfile.lock", "web/package.json"]
api-rails = ["infra/Dockerfile", "api/Gemfile", "api/Gemfile.lock", "api/package.json"]
```

`[assign]` セクションは、`coast assign` を実行して Coast インスタンスを別の worktree に切り替えるときに、各サービスで何が起きるかを制御します。ここを正しく設定できるかどうかが、ブランチ切り替えが 5 秒で終わるか 60 秒かの差になります。

### `default = "none"`

デフォルトを `"none"` に設定すると、`[assign.services]` に明示的に列挙されていないサービスはブランチ切り替え時に何もしません。これは DB やキャッシュにとって重要です — Postgres、Redis、インフラ系サービスはブランチ間で変わらず、再起動は無駄です。

### サービスごとの戦略

| Service | Strategy | Why |
|---|---|---|
| `web-rails`, `web-ssr`, `api-rails` | `hot` | これらはファイルウォッチャー付きの開発サーバーを動かします。[ファイルシステム再マウント](../concepts_and_terminology/FILESYSTEM.md) により `/workspace` 配下のコードが入れ替わり、ウォッチャーが自動的に変更を検知します。コンテナ再起動は不要です。 |
| `web-sidekiq`, `api-sidekiq` | `restart` | バックグラウンドワーカーは起動時にコードを読み込み、ファイル変更を監視しません。新しいブランチのコードを取り込むにはコンテナの再起動が必要です。 |

実際に動いているサービスだけを列挙してください。`COMPOSE_PROFILES` で一部のサービスしか起動しない場合、非アクティブなものは列挙しないでください — Coast は列挙されたすべてのサービスに対して assign 戦略を評価し、動いていないサービスを再起動しようとするのは無駄です。詳細は [Performance Optimizations](../concepts_and_terminology/PERFORMANCE_OPTIMIZATIONS.md) を参照してください。

### `exclude_paths`

これは大規模モノレポにおける最も影響の大きい最適化です。新しい worktree の作成時に gitignore されたファイルを初回ブートストラップする際、ディレクトリツリー全体をスキップするよう Coast に指示します。

目的は、Coast のサービスが必要としないものをすべて除外することです。30,000 ファイルのモノレポでは、上記のディレクトリが 8,000+ ファイルを占め、それらは実行中のサービスには無関係かもしれません。除外することで、無視ファイルのブートストラップを Coast が実際に必要とするより小さなサブセットに集中させられます。

除外対象を見つけるには、リポジトリをプロファイルします:

```bash
git ls-files --others --ignored --exclude-standard | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

実行中サービスにマウントされるソースコード、これらのサービスがインポートする共有ライブラリ、または初回ブートでランタイムが必要とする生成ファイルを含むディレクトリは残してください。それ以外 — ドキュメント、CI 設定、ツール、他チームのアプリ、モバイルクライアント、CLI ツール、そして `.yarn` のような同梱キャッシュ — は除外します。

### `rebuild_triggers`

トリガーがない場合、`strategy = "rebuild"` のサービスはブランチ切り替えのたびに Docker イメージを再ビルドします — イメージに影響する変更が何もなくてもです。`[assign.rebuild_triggers]` セクションは、特定のファイルに基づいて再ビルドをゲートします。

このレシピでは、Rails サービスは通常 `"hot"`（再起動すらしない）を使います。しかし誰かが Dockerfile や Gemfile を変更した場合、`rebuild_triggers` が作動してフルのイメージ再ビルドを強制します。トリガーファイルがどれも変更されていないなら、Coast は再ビルドを完全にスキップします。これにより、日常的なコード変更では高価なイメージビルドを避けつつ、インフラレベルの変更は確実に反映できます。

## Secrets と Inject

```toml
[secrets.compose_profiles]
extractor = "command"
run = "echo api,web"
inject = "env:COMPOSE_PROFILES"

[secrets.uid]
extractor = "command"
run = "id -u"
inject = "env:UID"

[secrets.gid]
extractor = "command"
run = "id -g"
inject = "env:GID"

[inject]
env = ["USER", "BUNDLE_GEMS__CONTRIBSYS__COM"]
```

`[secrets]` セクションはビルド時に値を抽出し、環境変数として Coast インスタンスへ注入します。

- **`compose_profiles`** は開始する Docker Compose プロファイルを制御します。compose ファイルに定義されているすべてのサービスではなく `api` と `web` プロファイルだけを動かすよう Coast を制限する方法です。ホスト側でビルド前に `export COMPOSE_PROFILES=api,web,portal` として上書きすれば、開始するサービスを変更できます。
- **`uid` / `gid`** はホストユーザーの UID と GID をコンテナに渡します。これはホストとコンテナ間でファイル所有者を一致させる必要がある Docker セットアップで一般的です。

`[inject]` セクションはより単純で、既存のホスト環境変数を実行時に Coast コンテナへ転送します。gem サーバートークン（`BUNDLE_GEMS__CONTRIBSYS__COM`）のような機密資格情報はホストに残り、どの設定ファイルにも書き込まれることなく転送されます。

シークレット抽出器と注入先の完全なリファレンスは [Secrets](../coastfiles/SECRETS.md) を参照してください。

## このレシピの適用

**別の言語スタック:** Rails 固有のボリューム（bundle、rails cache、assets）を、そのスタックに相当するもの（Go の module キャッシュ `/go/pkg/mod`、npm キャッシュ、pip キャッシュなど）に置き換えてください。インスタンス間で共有して安全なキャッシュであれば、戦略は `"shared"` のままで構いません。

**アプリが少ない:** モノレポにアプリが1つしかない場合は、余分なボリュームエントリを削除し、`[assign.services]` を自分のサービスだけに簡素化してください。共有サービスとベアサービスのパターンは引き続き適用できます。

**インスタンスごとの DB:** Coast インスタンス間でデータ分離が必要なら、`[shared_services.db]` を compose 内部の Postgres に置き換え、`strategy = "isolated"` の `[volumes]` エントリを追加してください。各インスタンスは専用の DB ボリュームを持ちます。`snapshot_source` を使ってホストのボリュームからシードできます — 詳細は [Volumes Coastfile reference](../coastfiles/VOLUMES.md) を参照してください。

**ベアサービスなし:** フロントエンドが完全にコンテナ化されていて `host.docker.internal` 経由で到達できる必要がない場合は、`[services.*]` セクションと `[coast.setup]` を削除してください。すべてが compose 経由で実行されます。
