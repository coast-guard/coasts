# 動的ポート環境変数

すべての Coast インスタンスは、各サービスに割り当てられた[動的ポート](PORTS.md)を公開する一連の環境変数を取得します。これらの変数は、素のサービスと compose コンテナの両方の内部で利用でき、アプリケーションが実行時に外部から到達可能なポートを検出できるようにします。

## 命名規則

Coast は、`[ports]` セクション内の論理サービス名から変数名を導出します。

1. 大文字に変換する
2. 英数字以外の文字をアンダースコアに置き換える
3. `_DYNAMIC_PORT` を付加する

```text
[ports] key          Environment variable
─────────────        ────────────────────────────
web             →    WEB_DYNAMIC_PORT
postgres        →    POSTGRES_DYNAMIC_PORT
backend-test    →    BACKEND_TEST_DYNAMIC_PORT
svc.v2          →    SVC_V2_DYNAMIC_PORT
```

サービス名が数字で始まる場合、Coast は変数名の先頭にアンダースコアを付けます（例: `9svc` は `_9SVC_DYNAMIC_PORT` になります）。空の名前は `SERVICE_DYNAMIC_PORT` にフォールバックします。

## 例

次の Coastfile があるとします。

```toml
[ports]
web = 3000
api = 8080
postgres = 5432
```

このビルドから作成されるすべての Coast インスタンスには、3 つの追加の環境変数があります。

```text
WEB_DYNAMIC_PORT=62217
API_DYNAMIC_PORT=55681
POSTGRES_DYNAMIC_PORT=56905
```

実際のポート番号は `coast run` 時に割り当てられ、インスタンスごとに異なります。

## いつ使うか

最も一般的なユースケースは、レスポンス内に独自の URL を埋め込むサービスの設定です。たとえば、認証コールバック、OAuth リダイレクト URI、CORS オリジン、Webhook URL などです。これらのサービスは、自身が待ち受けている内部ポートではなく、外部クライアントが使用するポートを知る必要があります。

たとえば、NextAuth を使用する Next.js アプリケーションでは、`AUTH_URL` を外部から到達可能なアドレスに設定する必要があります。Coast 内部では、Next.js は常にポート 3000 で待ち受けますが、ホスト側のポートは動的です。

```toml
[services.web]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} yarn dev:web"
port = 3000
```

`:-3000` のフォールバックにより、`WEB_DYNAMIC_PORT` が設定されていない Coast の外部でもこのコマンドは動作します。

## 優先順位

同じ名前の環境変数がすでに Coast コンテナ内に存在する場合（secrets、inject、または compose environment を通じて設定されている場合）、Coast はそれを上書きしません。既存の値が優先されます。

## 利用可能性

動的ポート変数は、起動時に Coast コンテナの環境へ注入されます。これらは次の場所で利用できます。

- 素のサービスの `install` コマンド
- 素のサービスの `command` プロセス
- Compose サービスコンテナ（コンテナ環境経由）
- `coast exec` を通じて実行されるコマンド

これらの値は、インスタンスの存続期間中は変化しません。インスタンスを停止して再起動しても、同じ動的ポートを維持します。

## 関連項目

- [Ports](PORTS.md) - 正式ポートと動的ポート、および checkout がそれらをどのように切り替えるか
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - サブドメインルーティングとインスタンス間の Cookie 分離
