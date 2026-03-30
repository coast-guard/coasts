# ベアサービス

プロジェクトをコンテナ化できるなら、そうすべきです。ベアサービスは、まだコンテナ化されておらず、短期的に `Dockerfile` と `docker-compose.yml` を追加するのが現実的ではないプロジェクトのために存在します。

コンテナ化されたサービスをオーケストレーションする `docker-compose.yml` の代わりに、ベアサービスでは Coastfile にシェルコマンドを定義し、Coast がそれらを Coast コンテナ内の軽量なスーパーバイザーで素のプロセスとして実行します。

## 代わりにコンテナ化すべき理由

[Docker Compose](RUNTIMES_AND_SERVICES.md) サービスが提供するもの:

- Dockerfile による再現可能なビルド
- 起動時に Coast が待機できるヘルスチェック
- サービス間のプロセス分離
- Docker によって処理されるボリュームとネットワーク管理
- CI、ステージング、本番で動作するポータブルな定義

ベアサービスにはこれらが一切ありません。プロセスは同じファイルシステムを共有し、クラッシュリカバリーはシェルループであり、「自分のマシンでは動く」は Coast の中でも外でも同じくらい起こり得ます。プロジェクトにすでに `docker-compose.yml` があるなら、それを使ってください。

## ベアサービスが有用な場合

- 一度もコンテナ化されたことのないプロジェクトで Coast を採用しており、worktree の分離とポート管理からすぐに価値を得たい
- Dockerfile が過剰になりがちな単一プロセスのツールや CLI である
- コンテナ化を段階的に進めたい。ベアサービスから始めて、後で compose に移行したい

## 設定

ベアサービスは Coastfile 内の `[services.<name>]` セクションで定義します。Coastfile はベアサービスのみを定義することも、`compose` と並べて定義することもできます。後者については [Mixed Service Types](MIXED_SERVICE_TYPES.md) を参照してください。

```toml
[coast]
name = "my-app"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
web = 3000
```

各サービスには 4 つのフィールドがあります:

| Field | Required | Description |
|---|---|---|
| `command` | yes | 実行するシェルコマンド（例: `"npm run dev"`） |
| `port` | no | サービスが待ち受けるポート。ポートマッピングに使用 |
| `restart` | no | 再起動ポリシー: `"no"`（デフォルト）、`"on-failure"`、または `"always"` |
| `install` | no | 起動前に実行する 1 つ以上のコマンド（例: `"npm install"` または `["npm install", "npm run build"]`） |

### セットアップパッケージ

ベアサービスは素のプロセスとして動作するため、Coast コンテナに適切なランタイムがインストールされている必要があります。`[coast.setup]` を使ってシステムパッケージを宣言します:

```toml
[coast.setup]
packages = ["nodejs", "npm"]
```

これらは、どのサービスが開始される前にもインストールされます。これがないと、コンテナ内で `npm` や `node` コマンドが失敗します。

### インストールコマンド

`install` フィールドは、サービス開始前に実行され、さらに毎回 [`coast assign`](ASSIGN.md)（ブランチ切り替え）時にも再実行されます。依存関係のインストールはここに置きます:

```toml
[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
```

インストールコマンドは順番に実行されます。いずれかのインストールコマンドが失敗した場合、そのサービスは開始されません。

### 再起動ポリシー

- **`no`**: サービスは 1 回だけ実行されます。終了したら、そのまま停止した状態になります。ワンショットのタスクや手動で管理したいサービスに使用します。
- **`on-failure`**: 非ゼロコードで終了した場合にサービスを再起動します。正常終了（コード 0）はそのままにします。1 秒から最大 30 秒までの指数バックオフを使用し、10 回連続でクラッシュすると諦めます。
- **`always`**: 成功を含むあらゆる終了で再起動します。`on-failure` と同じバックオフです。決して止まってほしくない長時間稼働のサーバーに使用します。

クラッシュする前にサービスが 30 秒以上稼働していた場合、リトライカウンターとバックオフはリセットされます。しばらくは健全だったとみなし、そのクラッシュは新しい問題だという前提です。

## How It Works

```text
┌─── Coast: dev-1 ──────────────────────────────────────┐
│                                                       │
│   /coast-supervisor/                                  │
│   ├── web.sh          (runs command, tracks PID)      │
│   ├── worker.sh                                       │
│   ├── start-all.sh    (launches all services)         │
│   ├── stop-all.sh     (SIGTERM via PID files)         │
│   └── ps.sh           (checks PID liveness)           │
│                                                       │
│   /var/log/coast-services/                            │
│   ├── web.log                                         │
│   └── worker.log                                      │
│                                                       │
│   No inner Docker daemon images are used.             │
│   Processes run directly on the container OS.         │
└───────────────────────────────────────────────────────┘
```

Coast は各サービス用のシェルスクリプトラッパーを生成し、DinD コンテナ内の `/coast-supervisor/` に配置します。各ラッパーは PID を追跡し、出力をログファイルにリダイレクトし、再起動ポリシーをシェルループとして実装します。Docker Compose はなく、内部の Docker イメージもなく、サービス間のコンテナレベルの分離もありません。

`coast ps` は Docker に問い合わせるのではなく PID の生存確認を行い、`coast logs` は `docker compose logs` を呼ぶのではなくログファイルを tail します。ログ出力形式は compose の `service | line` 形式に一致するため、Coastguard の UI は変更なしで動作します。

## ポート

ポート設定は compose ベースの Coast とまったく同じように動作します。サービスが待ち受けるポートを `[ports]` で定義します:

```toml
[services.web]
command = "npm start"
port = 3000

[ports]
web = 3000
```

[Dynamic ports](PORTS.md) は `coast run` 時に割り当てられ、[`coast checkout`](CHECKOUT.md) は通常どおりカノニカルポートを入れ替えます。唯一の違いは、サービス間に Docker ネットワークがないことです — すべてのサービスはコンテナのループバックまたは `0.0.0.0` に直接バインドします。

## ブランチ切り替え

ベアサービスの Coast で `coast assign` を実行すると、次のことが起こります:

1. 実行中の全サービスが SIGTERM により停止される
2. worktree が新しいブランチに切り替わる
3. インストールコマンドが再実行される（例: `npm install` が新しいブランチの依存関係を取り込む）
4. 全サービスが再起動する

これは compose で起こること — `docker compose down`、ブランチ切り替え、再ビルド、`docker compose up` — と同等ですが、コンテナの代わりにシェルプロセスを使用します。

## 制限事項

- **ヘルスチェックなし。** Coast は、ヘルスチェックを定義した compose サービスのように、ベアサービスが「healthy」になるのを待てません。Coast はプロセスを起動しますが、準備完了になったタイミングを知る手段がありません。
- **サービス間の分離なし。** すべてのプロセスは Coast コンテナ内で同じファイルシステムとプロセス名前空間を共有します。問題のあるサービスが他に影響する可能性があります。
- **ビルドキャッシュなし。** Docker Compose のビルドはレイヤーごとにキャッシュされます。ベアサービスの `install` コマンドは assign のたびに最初から実行されます。
- **クラッシュリカバリーは基本的。** 再起動ポリシーは指数バックオフを備えたシェルループを使用します。systemd や supervisord のようなプロセススーパーバイザーではありません。
- **サービスに対する `[omit]` や `[unset]` がない。** Coastfile の型合成は compose サービスでは動作しますが、ベアサービスでは型付き Coastfile によって個別のサービスを省略することはサポートされません。

## Compose への移行

コンテナ化の準備ができたら、移行パスは簡単です:

1. 各サービスの `Dockerfile` を書く
2. それらを参照する `docker-compose.yml` を作成する
3. Coastfile の `[services.*]` セクションを、compose ファイルを指す `compose` フィールドに置き換える
4. Dockerfile によって扱われるようになった `[coast.setup]` のパッケージを削除する
5. [`coast build`](BUILDS.md) で再ビルドする

ポートマッピング、[volumes](VOLUMES.md)、[shared services](SHARED_SERVICES.md)、[secrets](SECRETS.md) の設定はすべて変更なしで引き継がれます。変わるのは、サービス自体の実行方法だけです。
