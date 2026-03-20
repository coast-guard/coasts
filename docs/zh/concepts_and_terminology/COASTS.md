# Coast

Coast 是你项目的一个自包含运行时。它运行在一个 [Docker-in-Docker 容器](RUNTIMES_AND_SERVICES.md) 内，并且多个服务（你的 Web 服务器、数据库、缓存等）都可以在单个 Coast 实例中运行。

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

每个 Coast 都会向主机暴露自己的一组 [动态端口](PORTS.md)，这意味着无论还有什么在运行，你都可以随时访问任何正在运行的 Coast。

当你 [签出](CHECKOUT.md) 一个 Coast 时，项目的规范端口会被映射到它——因此 `localhost:3000` 会命中已签出的 Coast，而不是某个动态端口。

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

通常，一个 Coast 会[分配给特定的工作树](ASSIGN.md)。这就是你如何并行运行同一项目的多个工作树，而不会发生端口冲突或卷冲突。

你可以使用 [`coast run`](RUN.md) 创建 Coast 实例。至于何时启动或关闭 Coast，则由你自行决定。你大概不会想让一个内存密集型项目的 20 个 Coast 同时运行，不过各人有各人的选择。
