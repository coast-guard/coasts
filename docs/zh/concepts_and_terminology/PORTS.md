# 端口

Coast 会为一个 Coast 实例中的每个服务管理两种端口映射:规范端口和动态端口。

## 规范端口

这些是你的项目通常运行所使用的端口——也就是你在 `docker-compose.yml` 或本地开发配置中的那些端口。例如，Web 服务器的 `3000`，Postgres 的 `5432`。

同一时间只能有一个 Coast 拥有规范端口。哪个 Coast 被[检出](CHECKOUT.md)，哪个就会获得这些端口。

```text
coast checkout dev-1

localhost:3000  ──→  dev-1
localhost:5432  ──→  dev-1
```

这意味着你的浏览器、API 客户端、数据库工具和测试套件都会像平常一样工作——无需更改端口号。

在 Linux 上，低于 `1024` 的规范端口在 [`coast checkout`](CHECKOUT.md) 绑定它们之前，可能需要先进行主机配置。动态端口没有这个限制。

## 动态端口

每个正在运行的 Coast 始终都会在高位端口范围（49152–65535）内获得自己的一组动态端口。这些端口会被自动分配，并且无论当前检出的是哪个 Coast，它们都始终可访问。

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

动态端口让你无需检出就能查看任意 Coast。你可以打开 `localhost:63104` 访问 dev-2 的 Web 服务器，同时 dev-1 仍然占用规范端口被检出。

## 它们如何协同工作

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

切换[检出](CHECKOUT.md)是即时的。Coast 会终止并重新生成轻量级的 `socat` 转发器。不会重启任何容器。

## 动态端口环境变量

Coast 会向每个实例注入环境变量，以暴露每个服务的动态端口。变量名由 `[ports]` 键派生而来:`web` 会变成 `WEB_DYNAMIC_PORT`，`backend-test` 会变成 `BACKEND_TEST_DYNAMIC_PORT`。

当某个服务需要知道其可从外部访问的端口时，这些变量会很有用，例如为认证回调重定向设置 `AUTH_URL`。完整参考请见[动态端口环境变量](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md)。

## 端口与远程 Coast

对于[远程 Coast](REMOTES.md)，端口会额外经过一层 SSH 隧道。每个本地动态端口都会通过 `ssh -L` 转发到对应的远程动态端口，而远程动态端口又会映射到远程 DinD 容器内的规范端口。这一过程是透明的——`coast ports` 和 `coast checkout` 对本地实例和远程实例的工作方式完全相同。

## 另请参阅

- [主端口与 DNS](PRIMARY_PORT_AND_DNS.md) - 快速链接、子域路由和 URL 模板
- [动态端口环境变量](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - 在服务命令中使用 `WEB_DYNAMIC_PORT` 及相关变量
- [远程](REMOTES.md) - 远程 Coast 的端口转发工作原理
