# Coasts

Coast は、プロジェクトの自己完結型のランタイムです。[Docker-in-Docker コンテナ](RUNTIMES_AND_SERVICES.md)内で実行され、複数のサービス（Web サーバー、データベース、キャッシュなど）をすべて 1 つの Coast インスタンス内で実行できます。

```text
┌─── Coast: dev-1 (branch: feature/oauth) ──────────────┐
│                                                       │
│   ┌─────────┐   ┌──────────┐   ┌─────────┐            │
│   │   web   │   │ postgres │   │  redis  │            │
│   │  :3000  │   │  :5432   │   │  :6379  │            │
│   └─────────┘   └──────────┘   └─────────┘            │
│                                                       │
│   dynamic ports: 62217, 55681, 56905                  │
└───────────────────────────────────────────────────────┘

┌─── Coast: dev-2 (branch: feature/billing) ────────────┐
│                                                       │
│   ┌─────────┐   ┌──────────┐   ┌─────────┐            │
│   │   web   │   │ postgres │   │  redis  │            │
│   │  :3000  │   │  :5432   │   │  :6379  │            │
│   └─────────┘   └──────────┘   └─────────┘            │
│                                                       │
│   dynamic ports: 63104, 57220, 58412                  │
└───────────────────────────────────────────────────────┘
```

各 Coast はホストマシンに対してそれぞれ独自の [dynamic ports](PORTS.md) セットを公開します。つまり、他に何が動いているかに関係なく、実行中のどの Coast にもいつでもアクセスできます。

Coast を [check out](CHECKOUT.md) すると、プロジェクトの標準（canonical）ポートがその Coast にマッピングされます — そのため `localhost:3000` は dynamic port ではなく、check out された Coast にアクセスします。

```text
coast checkout dev-1

localhost:3000  ──→  dev-1 web
localhost:5432  ──→  dev-1 postgres
localhost:6379  ──→  dev-1 redis

coast checkout dev-2   (instant swap)

localhost:3000  ──→  dev-2 web
localhost:5432  ──→  dev-2 postgres
localhost:6379  ──→  dev-2 redis
```

通常、Coast は[特定の worktree に割り当てられます](ASSIGN.md)。これにより、同じプロジェクトの複数の worktree を、ポートの競合やボリュームの衝突なしに並列で実行できます。

Coast を起動・停止するかどうかは、必要に応じてあなたが決めます。メモリ消費の大きいプロジェクトで 20 個の Coast を同時に動かしたいとはおそらく思わないでしょうが、それは人それぞれです。
