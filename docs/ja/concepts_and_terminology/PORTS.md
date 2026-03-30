# ポート

Coast は、Coast インスタンス内のすべてのサービスに対して、2 種類のポートマッピングを管理します: canonical ports と dynamic ports です。

## Canonical Ports

これらは、あなたのプロジェクトが通常実行されるポートです — `docker-compose.yml` やローカル開発設定にあるものです。たとえば、Web サーバーなら `3000`、Postgres なら `5432` です。

同時に canonical ports を持てる Coast は 1 つだけです。[チェックアウト](CHECKOUT.md) されている Coast がそれらを取得します。

```text
coast checkout dev-1

localhost:3000  ──→  dev-1
localhost:5432  ──→  dev-1
```

これは、ブラウザ、API クライアント、データベースツール、テストスイートのすべてが、通常どおり正確に動作することを意味します — ポート番号を変更する必要はありません。

Linux では、`1024` 未満の canonical ports は、[`coast checkout`](CHECKOUT.md) がそれらにバインドできるようになる前に、ホストの設定が必要になる場合があります。dynamic ports にはこの制限はありません。

## Dynamic Ports

実行中のすべての Coast は、それぞれ高い範囲 (49152–65535) の dynamic ports の独自セットを常に取得します。これらは自動的に割り当てられ、どの Coast がチェックアウトされているかに関係なく、常にアクセス可能です。

```text
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681

coast ports dev-2

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       63104
#   db       5432       57220
```

dynamic ports を使うと、チェックアウトせずに任意の Coast をのぞき見ることができます。canonical ports では dev-1 がチェックアウトされている間でも、`localhost:63104` を開いて dev-2 の Web サーバーにアクセスできます。

## それらがどのように連携するか

```text
┌──────────────────────────────────────────────────┐
│  Your machine                                    │
│                                                  │
│  Canonical (checked-out Coast only):             │
│    localhost:3000 ──→ dev-1 web                  │
│    localhost:5432 ──→ dev-1 db                   │
│                                                  │
│  Dynamic (always available):                     │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

[checkout](CHECKOUT.md) の切り替えは一瞬です。Coast は軽量な `socat` フォワーダーを停止して再生成します。コンテナが再起動されることはありません。

## Dynamic Port Environment Variables

Coast は、各サービスの dynamic port を公開する環境変数をすべてのインスタンスに注入します。変数名は `[ports]` キーから導出されます: `web` は `WEB_DYNAMIC_PORT` になり、`backend-test` は `BACKEND_TEST_DYNAMIC_PORT` になります。

これらは、サービスが外部から到達可能なポートを知る必要がある場合に便利です。たとえば、認証コールバックのリダイレクト用に `AUTH_URL` を設定する場合などです。完全なリファレンスについては、[Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) を参照してください。

## See Also

- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - quick-links, subdomain routing, and URL templates
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - using `WEB_DYNAMIC_PORT` and related variables in service commands
