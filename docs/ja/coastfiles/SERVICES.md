# ベアサービス

> **注:** ベアサービスは、プレーンなプロセスとして Coast コンテナ内で直接実行されます — コンテナ化されません。サービスがすでに Docker 化されている場合は、代わりに `compose` を使用してください。ベアサービスは、Dockerfile や docker-compose.yml を書くオーバーヘッドを省きたいシンプルな構成に最適です。

`[services.*]` セクションは、Coast が Docker Compose を使わずに DinD コンテナ内で直接実行するプロセスを定義します。これは `compose` ファイルを使う代替手段です — 同じ Coastfile 内で両方を使うことはできません。

ベアサービスは Coast によって監督され、ログの取得と任意の再起動ポリシーが提供されます。ベアサービスの動作方法、制限、そして compose へ移行すべきタイミングについてのより深い背景は、[Bare Services](../concepts_and_terminology/BARE_SERVICES.md) を参照してください。

## サービスの定義

各サービスは `[services]` 配下の名前付き TOML セクションです。`command` フィールドは必須です。

```toml
[services.web]
command = "node server.js"
port = 3000
```

### `command`（必須）

実行するシェルコマンド。空または空白のみであってはなりません。

```toml
[services.web]
command = "npx next dev --turbopack --port 3000 --hostname 0.0.0.0"
```

### `port`

サービスがリッスンするポート。ヘルスチェックおよびポートフォワーディング連携に使用されます。指定する場合は 0 以外でなければなりません。

```toml
[services.web]
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

### `restart`

プロセスが終了した場合の再起動ポリシー。デフォルトは `"no"` です。

- `"no"` — 再起動しない
- `"on-failure"` — プロセスが非ゼロコードで終了した場合のみ再起動する
- `"always"` — 常に再起動する

```toml
[services.web]
command = "node server.js"
port = 3000
restart = "on-failure"
```

### `install`

サービス開始前に実行するコマンド（例: 依存関係のインストール）。単一の文字列または文字列配列を受け付けます。

```toml
[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

```toml
[services.web]
install = ["npm install", "npm run build"]
command = "npm start"
port = 3000
```

## compose との相互排他

Coastfile は `compose` と `[services]` の両方を定義できません。`[coast]` に `compose` フィールドがある場合、任意の `[services.*]` セクションを追加するとエラーになります。Coastfile ごとにどちらか一方のアプローチを選んでください。

compose 経由でコンテナ化するサービスと、ベアで動かすサービスを混在させたい場合でも、すべて compose を使用してください — ベアサービスから compose へ移行する方法は、[Bare Services の移行ガイダンス](../concepts_and_terminology/BARE_SERVICES.md) を参照してください。

## 例

### 単一サービスの Next.js アプリ

```toml
[coast]
name = "my-frontend"

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

### バックグラウンドワーカー付き Web サーバー

```toml
[coast]
name = "my-app"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "node server.js"
port = 3000
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
web = 3000
```

### 複数ステップの install を伴う Python サービス

```toml
[coast]
name = "ml-service"

[coast.setup]
packages = ["python3", "py3-pip"]

[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
restart = "on-failure"

[ports]
api = 8000
```
