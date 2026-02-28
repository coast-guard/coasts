# ベアサービス

プロジェクトをコンテナ化できるなら、そうするべきです。ベアサービスは、まだコンテナ化されておらず、短期的に `Dockerfile` と `docker-compose.yml` を追加するのが現実的ではないプロジェクトのために存在します。これは踏み石であり、目的地ではありません。

コンテナ化されたサービスを `docker-compose.yml` でオーケストレーションする代わりに、ベアサービスでは Coastfile にシェルコマンドを定義でき、Coast はそれらを Coast コンテナ内の軽量なスーパーバイザーによって、単なるプロセスとして実行します。

## なぜ代わりにコンテナ化するべきか

[Docker Compose](RUNTIMES_AND_SERVICES.md) サービスは、次のものを提供します:

- Dockerfile による再現可能なビルド
- 起動時に Coast が待機できるヘルスチェック
- サービス間のプロセス分離
- Docker によって処理されるボリュームとネットワーク管理
- CI、ステージング、本番で動作する移植可能な定義

ベアサービスはそのいずれも提供しません。プロセスは同じファイルシステムを共有し、クラッシュリカバリーはシェルループであり、「自分のマシンでは動く」は Coast の内側でも外側でも同じくらい起こりえます。プロジェクトにすでに `docker-compose.yml` があるなら、それを使ってください。

## ベアサービスが意味を持つ場合

- これまで一度もコンテナ化されていないプロジェクトに Coast を導入しており、worktree の分離とポート管理の価値をすぐに得たい
- プロジェクトが単一プロセスのツールや CLI で、Dockerfile が過剰になる
- コンテナ化を段階的に進めたい — ベアサービスから始め、後で compose に移行する

## 設定

ベアサービスは Coastfile の `[services.<name>]` セクションで定義します。Coastfile は `compose` と `[services]` の両方を定義することは **できません** — これらは相互排他です。

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

ベアサービスは単なるプロセスとして実行されるため、Coast コンテナには適切なランタイムがインストールされている必要があります。`[coast.setup]` を使ってシステムパッケージを宣言します:

```toml
[coast.setup]
packages = ["nodejs", "npm"]
```

これらはどのサービスの起動よりも前にインストールされます。これがないと、コンテナ内で `npm` や `node` コマンドが失敗します。

### インストールコマンド

`install` フィールドはサービス起動前に実行され、さらに毎回 [`coast assign`](ASSIGN.md)（ブランチ切り替え）時にも再実行されます。依存関係のインストールはここに置きます:

```toml
[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
```

インストールコマンドは順番に実行されます。いずれかのインストールコマンドが失敗すると、サービスは起動しません。

### 再起動ポリシー

- **`no`** — サービスは 1 回だけ実行されます。終了したら、そのまま停止したままになります。ワンショットのタスクや、手動で管理したいサービスに使います。
- **`on-failure`** — 非ゼロのコードで終了した場合にサービスを再起動します。正常終了（コード 0）はそのままにします。1 秒から最大 30 秒までの指数バックオフを使用し、10 回連続でクラッシュすると諦めます。
- **`always`** — 成功も含め、あらゆる終了で再起動します。バックオフは `on-failure` と同じです。止まってほしくない長時間稼働のサーバーに使います。

サービスがクラッシュする前に 30 秒以上動作していた場合、リトライカウンタとバックオフはリセットされます — しばらくは健全だったとみなし、クラッシュは新しい問題だという前提です。

## 内部ではどのように動くか

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

Coast は各サービス用のシェルスクリプトのラッパーを生成し、DinD コンテナ内の `/coast-supervisor/` に配置します。各ラッパーは PID を追跡し、出力をログファイルへリダイレクトし、再起動ポリシーをシェルループとして実装します。Docker Compose はなく、内側の Docker イメージもなく、サービス間のコンテナレベルの分離もありません。

`coast ps` は Docker に問い合わせる代わりに PID の生存をチェックし、`coast logs` は `docker compose logs` を呼ぶ代わりにログファイルを tail します。ログ出力形式は compose の `service | line` 形式に一致するため、Coastguard の UI は変更なしで動作します。

## ポート

ポート設定は compose ベースの Coast とまったく同じように動作します。サービスが待ち受けるポートを `[ports]` に定義します:

```toml
[services.web]
command = "npm start"
port = 3000

[ports]
web = 3000
```

[Dynamic ports](PORTS.md) は `coast run` で割り当てられ、[`coast checkout`](CHECKOUT.md) は通常どおり canonical ポートをスワップします。唯一の違いは、サービス間に Docker ネットワークが存在しないことです — すべてのサービスはコンテナのループバックまたは `0.0.0.0` に直接バインドします。

## ブランチ切り替え

ベアサービスの Coast で `coast assign` を実行すると、次のことが起こります:

1. 実行中のサービスはすべて SIGTERM により停止される
2. worktree が新しいブランチに切り替わる
3. インストールコマンドが再実行される（例: `npm install` が新しいブランチの依存関係を取得する）
4. すべてのサービスが再起動する

これは compose で起こること — `docker compose down`、ブランチ切り替え、リビルド、`docker compose up` — と同等ですが、コンテナの代わりにシェルプロセスを使います。

## 制限事項

- **ヘルスチェックなし。** Coast は、ヘルスチェックを定義した compose サービスのように、ベアサービスが「healthy」になるのを待てません。プロセスを開始して、うまくいくことを祈るだけです。
- **サービス間の分離なし。** すべてのプロセスは Coast コンテナ内で同じファイルシステムとプロセス名前空間を共有します。問題のあるサービスが他に影響を与える可能性があります。
- **ビルドキャッシュなし。** Docker Compose のビルドはレイヤーごとにキャッシュされます。ベアサービスの `install` コマンドは assign のたびに最初から実行されます。
- **クラッシュリカバリーは基本的。** 再起動ポリシーは指数バックオフ付きのシェルループを使用します。systemd や supervisord のようなプロセススーパーバイザーではありません。
- **サービスに対する `[omit]` や `[unset]` がない。** Coastfile の型合成は compose サービスでは機能しますが、ベアサービスでは型付き Coastfile によって個別サービスを省略することはサポートされていません。

## Compose への移行

コンテナ化の準備ができたら、移行手順はシンプルです:

1. 各サービスの `Dockerfile` を書く
2. それらを参照する `docker-compose.yml` を作成する
3. Coastfile の `[services.*]` セクションを、compose ファイルを指す `compose` フィールドに置き換える
4. いまや Dockerfile が扱うようになった `[coast.setup]` のパッケージを削除する
5. [`coast build`](BUILDS.md) でリビルドする

ポートマッピング、[volumes](VOLUMES.md)、[shared services](SHARED_SERVICES.md)、および [secrets](SECRETS.md) の設定はすべて、変更なしで引き継がれます。変わるのは、サービスそのものの実行方法だけです。
