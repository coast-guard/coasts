# 端口

Coast 会为 Coast 实例中的每个服务管理两种端口映射:规范端口（canonical ports）和动态端口（dynamic ports）。

## 规范端口

这些是你的项目通常运行的端口——也就是你在 `docker-compose.yml` 或本地开发配置中的端口。例如，Web 服务器用 `3000`，Postgres 用 `5432`。

同一时间只能有一个 Coast 拥有规范端口。哪个 Coast 被 [检出](CHECKOUT.md)，哪个就获得它们。

```text
coast checkout dev-1

localhost:3000  ──→  dev-1
localhost:5432  ──→  dev-1
```

这意味着你的浏览器、API 客户端、数据库工具以及测试套件都会像平常一样正常工作——不需要修改任何端口号。

## 动态端口

每个正在运行的 Coast 都会在高端口范围（49152–65535）中获得自己的一组动态端口。这些端口会自动分配，并且始终可访问，不受哪个 Coast 被检出影响。

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

动态端口让你无需检出就能查看任意 Coast。即使 dev-1 以规范端口被检出，你也可以打开 `localhost:63104` 来访问 dev-2 的 Web 服务器。

## 它们如何协同工作

```text
┌──────────────────────────────────────────────────┐
│  你的机器                                        │
│                                                  │
│  规范端口（仅限被检出的 Coast）:                 │
│    localhost:3000 ──→ dev-1 web                  │
│    localhost:5432 ──→ dev-1 db                   │
│                                                  │
│  动态端口（始终可用）:                           │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

切换 [检出](CHECKOUT.md) 是即时的——Coast 会杀掉并重新生成轻量级的 `socat` 转发器。不会重启任何容器。

另请参阅 [主端口与 DNS](PRIMARY_PORT_AND_DNS.md)，了解快速链接、子域名路由以及 URL 模板。
