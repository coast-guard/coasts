# Bare Service の最適化

[Bare services](BARE_SERVICES.md) は、Coast コンテナ内でプレーンなプロセスとして実行されます。Docker レイヤーやイメージキャッシュがないため、起動およびブランチ切り替えのパフォーマンスは、`install` コマンド、キャッシュ、assign 戦略をどのように構成するかに依存します。

## 高速な Install コマンド

`install` フィールドは、サービスの起動前と、`coast assign` のたびに再び実行されます。`install` が無条件に `make` や `yarn install` を実行すると、何も変更されていない場合でも、ブランチを切り替えるたびに完全なインストールコストが発生します。

**可能な場合は条件チェックを使って処理をスキップしてください:**

```toml
[services.web]
install = "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)"
command = "cd /workspace && yarn dev:web"
```

`test -f` のガードにより、`node_modules` がすでに存在していれば install をスキップします。初回実行時やキャッシュミス後には、完全なインストールを実行します。その後の assign では依存関係が変更されていなければ、即座に完了します。

コンパイル済みバイナリの場合は、出力が存在するかを確認します:

```toml
[services.zoekt]
install = "cd /workspace && (test -f bin/zoekt-webserver || make zoekt)"
command = "cd /workspace && ./bin/zoekt-webserver -index .sourcebot/index -rpc"
```

## Worktree をまたいでディレクトリをキャッシュする

Coast が bare-service インスタンスを新しい worktree に切り替えると、`/workspace` マウントは別のディレクトリに変更されます。`node_modules` やコンパイル済みバイナリのようなビルド成果物は、古い worktree に残されたままになります。`cache` フィールドは、切り替えをまたいで保持するディレクトリを Coast に指定します:

```toml
[services.web]
install = "cd /workspace && yarn install"
command = "cd /workspace && yarn dev"
cache = ["node_modules"]

[services.api]
install = "cd /workspace && make build"
command = "cd /workspace && ./bin/api-server"
cache = ["bin"]
```

キャッシュされたディレクトリは、worktree の再マウント前にバックアップされ、その後に復元されます。これにより、`yarn install` は最初からではなく増分的に実行され、コンパイル済みバイナリはブランチ切り替え後も維持されます。

## private_paths でインスタンスごとのディレクトリを分離する

一部のツールは、プロセスごとの状態を含むディレクトリを workspace 内に作成します。たとえば、ロックファイル、ビルドキャッシュ、PID ファイルなどです。複数の Coast インスタンスが同じ workspace（同じブランチ、worktree なし）を共有すると、これらのディレクトリは衝突します。

典型的な例は Next.js で、起動時に `.next/dev/lock` にロックを取得します。2 つ目の Coast インスタンスはそのロックを検出し、起動を拒否します。

`private_paths` は、指定したパスに対して各インスタンス専用の分離されたディレクトリを提供します:

```toml
[coast]
name = "my-app"
private_paths = ["packages/web/.next"]
```

各インスタンスは、そのパスにインスタンスごとのオーバーレイマウントを受け取ります。ロックファイル、ビルドキャッシュ、Turbopack の状態は完全に分離されます。コードの変更は不要です。

`private_paths` は、同じファイルに並行して書き込む複数インスタンスが問題を引き起こすあらゆるディレクトリに使用してください: `.next`、`.turbo`、`.parcel-cache`、PID ファイル、または SQLite データベース。

## Shared Services への接続

データベースやキャッシュに [shared services](SHARED_SERVICES.md) を使用する場合、shared コンテナは Coast 内ではなくホストの Docker デーモン上で実行されます。Coast 内で実行される bare service は、`localhost` 経由ではそれらに到達できません。

代わりに `host.docker.internal` を使用してください:

```toml
[services.web]
command = "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

また、[secrets](../coastfiles/SECRETS.md) を使って接続文字列を環境変数として注入することもできます:

```toml
[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"
```

Coast 内の Compose サービスにはこの問題はありません。Coast は compose コンテナ向けに、shared service のホスト名をブリッジネットワーク経由で自動的にルーティングします。これは bare service にのみ影響します。

## インライン環境変数

Bare service のコマンドは、`.env` ファイル、secrets、inject で設定されたものを含め、Coast コンテナから環境変数を継承します。しかし、共有設定ファイルを変更せずに、特定のサービスに対して単一の変数を上書きしたい場合があります。

コマンドの先頭にインライン代入を付けてください:

```toml
[services.web]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

インライン変数は、他のすべてよりも優先されます。これは次のような場合に便利です:

- `AUTH_URL` を [dynamic port](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) に設定して、チェックアウトされていないインスタンスでも認証リダイレクトが機能するようにする
- `DATABASE_URL` を上書きして、`host.docker.internal` 経由で shared service を指すようにする
- workspace 内の共有 `.env` ファイルを変更せずに、サービス固有のフラグを設定する

## Bare Service の Assign 戦略

各サービスがコード変更をどのように取り込むかに基づいて、適切な [assign strategy](../coastfiles/ASSIGN.md) を選択してください:

| Strategy | 使用するタイミング | 例 |
|---|---|---|
| `hot` | worktree の再マウント後に変更を自動検出するファイルウォッチャーをサービスが持っている | Next.js (HMR), Vite, webpack, nodemon, tsc --watch |
| `restart` | サービスが起動時にコードを読み込み、変更を監視しない | コンパイル済み Go バイナリ、Rails、Java サーバー |
| `none` | サービスが workspace のコードに依存しない、または別のインデックスを使う | データベースサーバー、Redis、検索インデックス |

```toml
[assign]
default = "none"

[assign.services]
web = "hot"
backend = "hot"
zoekt = "none"
```

デフォルトを `none` に設定すると、インフラ系サービスはブランチ切り替え時に一切変更されません。コード変更に関係するサービスだけが再起動されるか、ホットリロードに依存します。

## See Also

- [Bare Services](BARE_SERVICES.md) - bare services の完全なリファレンス
- [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md) - `exclude_paths` と `rebuild_triggers` を含む一般的なパフォーマンスチューニング
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - コマンド内での `WEB_DYNAMIC_PORT` および関連変数の使用方法
