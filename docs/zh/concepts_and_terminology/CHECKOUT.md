# Checkout

Checkout 用于控制哪个 Coast 实例拥有你项目的[规范端口](PORTS.md)。当你 checkout 到某个 Coast 时，`localhost:3000`、`localhost:5432` 以及其他所有规范端口都会直接映射到该实例。

```bash
coast checkout dev-1
```

```text
Before checkout:
  localhost:3000  ──→  (nothing)
  localhost:5432  ──→  (nothing)

After checkout:
  localhost:3000  ──→  dev-1 web
  localhost:5432  ──→  dev-1 db
```

切换 checkout 是瞬时的——Coast 会杀掉并重新生成轻量级的 `socat` 转发器。不会重启任何容器。

```bash
coast checkout dev-2   # instant swap

# localhost:3000  ──→  dev-2 web
# localhost:5432  ──→  dev-2 db
```

## Do You Need to Check Out?

不一定。每个正在运行的 Coast 始终都有自己的动态端口，并且你随时都可以通过这些端口访问任何 Coast，而无需 checkout 任何东西。

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

你可以在浏览器中打开 `localhost:62217` 来访问 dev-1 的 web 服务器，而无需将其 checkout。这对许多工作流来说完全没问题，而且你可以运行任意数量的 Coast，而从不使用 `coast checkout`。

## When Checkout Is Useful

在一些情况下，动态端口还不够，你需要规范端口:

- **客户端应用硬编码为规范端口。** 如果你有一个运行在 Coast 之外的客户端——例如宿主机上的前端开发服务器、手机上的移动应用，或桌面应用——它期望使用 `localhost:3000` 或 `localhost:8080`，那么到处修改端口号是不现实的。checkout 该 Coast 可以让你在不更改任何配置的情况下使用真实端口。

- **Webhook 和回调 URL。** 像 Stripe、GitHub 或 OAuth 提供方这样的服务会把回调发送到你注册的 URL——通常类似 `https://your-ngrok-tunnel.io`，它会转发到 `localhost:3000`。如果你切换到动态端口，回调就不会再到达。checkout 能确保你正在测试的 Coast 的规范端口处于激活状态。

- **数据库工具、调试器和 IDE 集成。** 许多 GUI 客户端（pgAdmin、DataGrip、TablePlus）、调试器以及 IDE 运行配置都会保存带有特定端口的连接配置。Checkout 让你可以保留已保存的配置文件，只需切换这些配置背后对应的 Coast——无需在每次切换上下文时重新配置调试器的 attach 目标或数据库连接。

## Releasing Checkout

如果你想释放规范端口，而不是 checkout 到另一个 Coast:

```bash
coast checkout --none
```

之后，没有任何 Coast 拥有规范端口。所有 Coast 仍可通过其动态端口访问。

## Only One at a Time

同一时间只能有一个 Coast 被 checkout。如果 `dev-1` 已被 checkout，而你运行 `coast checkout dev-2`，规范端口会立即切换到 `dev-2`。不会出现空档——旧的转发器会被杀掉，并在同一次操作中生成新的转发器。

```text
┌──────────────────────────────────────────────────┐
│  Your machine                                    │
│                                                  │
│  Canonical (checked-out Coast only):             │
│    localhost:3000 ──→ dev-2 web                  │
│    localhost:5432 ──→ dev-2 db                   │
│                                                  │
│  Dynamic (always available):                     │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

动态端口不受 checkout 影响。唯一变化的是规范端口指向哪里。
