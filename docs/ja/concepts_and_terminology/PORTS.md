# ポート

Coast は、Coast インスタンス内のすべてのサービスに対して 2 種類のポートマッピングを管理します: canonical ports と dynamic ports です。

## Canonical Ports

これらは、あなたのプロジェクトが通常実行されるポートです — `docker-compose.yml` やローカル開発設定にあるものです。たとえば、Web サーバーなら `3000`、Postgres なら `5432` です。

canonical ports を持てる Coast は同時に 1 つだけです。[チェックアウト](CHECKOUT.md) されている Coast がそれらを取得します。

```text
coast checkout dev-1

localhost:3000  ──→  dev-1
localhost:5432  ──→  dev-1
```

これは、ブラウザー、API クライアント、データベースツール、テストスイートのすべてが、通常どおり正確に動作することを意味します — ポート番号を変更する必要はありません。

Linux では、`1024` 未満の canonical ports は、[`coast checkout`](CHECKOUT.md) がそれらをバインドできるようにする前に、ホスト設定が必要になる場合があります。dynamic ports にはこの制限はありません。

## Dynamic Ports

実行中のすべての Coast には常に、高い範囲 (49152–65535) の独自の dynamic ports セットが割り当てられます。これらは自動的に割り当てられ、どの Coast がチェックアウトされているかに関係なく、常にアクセス可能です。

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

dynamic ports を使うと、チェックアウトせずに任意の Coast を確認できます。canonical ports で dev-1 がチェックアウトされている間でも、`localhost:63104` を開いて dev-2 の Web サーバーにアクセスできます。

## How They Work Together

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

[checkout](CHECKOUT.md) の切り替えは即時です。Coast は軽量な `socat` フォワーダーを停止して再生成します。コンテナは再起動されません。

## Dynamic Port Environment Variables

Coast は、各サービスの dynamic port を公開する環境変数をすべてのインスタンスに注入します。変数名は `[ports]` キーから導出されます: `web` は `WEB_DYNAMIC_PORT` になり、`backend-test` は `BACKEND_TEST_DYNAMIC_PORT` になります。

これらは、たとえば認証コールバックのリダイレクト用に `AUTH_URL` を設定するなど、サービスが外部から到達可能なポートを知る必要がある場合に便利です。完全なリファレンスについては、[Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) を参照してください。

## Ports and Remote Coasts

[remote coasts](REMOTES.md) では、ポートは追加の SSH トンネル層を経由します。各ローカル dynamic port は `ssh -L` によって対応するリモート dynamic port に転送され、さらにそれがリモート DinD コンテナ内の canonical port にマッピングされます。これは透過的です -- `coast ports` と `coast checkout` は、ローカルインスタンスでもリモートインスタンスでも同一に動作します。

## See Also

- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - クイックリンク、サブドメインルーティング、URL テンプレート
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - サービスコマンド内で `WEB_DYNAMIC_PORT` および関連変数を使用する方法
- [Remotes](REMOTES.md) - remote coasts でポートフォワーディングがどのように機能するか
