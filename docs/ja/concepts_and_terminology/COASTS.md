# Coast

Coast は、あなたのプロジェクトの自己完結型ランタイムです。[Docker-in-Docker コンテナ](RUNTIMES_AND_SERVICES.md)内で実行され、複数のサービス（Web サーバー、データベース、キャッシュなど）をすべて 1 つの Coast インスタンス内で実行できます。

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

各 Coast は、それぞれ独自の[動的ポート](PORTS.md)のセットをホストマシンに公開します。つまり、他に何が実行中であっても、いつでも任意の実行中の Coast にアクセスできます。

Coast を[チェックアウト](CHECKOUT.md)すると、プロジェクトの正規ポートがその Coast にマッピングされます。したがって、`localhost:3000` は動的ポートではなく、チェックアウトされた Coast にアクセスします。

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

通常、Coast は[特定の worktree に割り当て](ASSIGN.md)られます。これにより、同じプロジェクトの複数の worktree を、ポート競合やボリューム衝突なしに並行して実行できます。

Coast インスタンスは [`coast run`](RUN.md) で作成します。Coast をいつ起動し、いつ停止するかはあなた次第です。おそらく、メモリ消費の大きいプロジェクトで 20 個の Coast を同時に動かしたいとは思わないでしょうが、それぞれのやり方があります。
