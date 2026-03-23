# Checkout

Checkout 控制哪个 Coast 实例拥有你项目的[规范端口](PORTS.md)。当你 checkout 一个 Coast 时，`localhost:3000`、`localhost:5432` 以及其他所有规范端口都会直接映射到该实例。

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

切换 checkout 是即时完成的——Coast 会终止并重新生成轻量级的 `socat` 转发器。不会重启任何容器。

```bash
coast checkout dev-2   # instant swap

# localhost:3000  ──→  dev-2 web
# localhost:5432  ──→  dev-2 db
```

## Linux Note

动态端口在 Linux 上始终可用，不需要特殊权限。

低于 `1024` 的规范端口则不同。如果你的 Coastfile 声明了像 `80` 或 `443` 这样的端口，在你配置主机之前，Linux 可能会阻止 `coast checkout` 绑定这些端口。常见的修复方式有:

- 提高 `net.ipv4.ip_unprivileged_port_start`
- 向转发二进制文件或进程授予绑定能力

当主机拒绝绑定时，Coast 会明确报告这一点。

在 WSL 上，Coast 使用 Docker 发布的 checkout bridge，因此 Windows 浏览器和工具可以通过 `127.0.0.1` 访问已 checkout 的规范端口，这类似于 Docker Desktop 工作流（如 Sail）。

对于使用 Caddy 的本地 HTTPS 项目，Coast 会为每个 Coast 安装复用同一个 Caddy 本地 CA。你只需信任一次该根证书，在同一安装下重新创建的工作区就会继续使用它。

根证书位于:

- 常规安装为 `~/.coast/caddy/pki/authorities/local/root.crt`
- `coast-dev` 为 `~/.coast-dev/caddy/pki/authorities/local/root.crt`

它们是有意分开的，因此信任 `coast-dev` 不会同时信任常规的 `coast` 安装，反之亦然。

要查看或导出当前安装的根证书:

```bash
coast cert info
coast cert path
coast cert fingerprint
coast cert export --to ~/Downloads/coast-root.crt
```

Coast 将信任安装交由你自己处理。导出证书后，再按需将其导入你的操作系统或浏览器信任存储中。

## Do You Need to Check Out?

不一定。每个正在运行的 Coast 始终都有自己的动态端口，而且你随时都可以通过这些端口访问任意 Coast，而无需 checkout 任何内容。

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

你可以在浏览器中打开 `localhost:62217` 来访问 dev-1 的 Web 服务器，而无需 checkout。这对于许多工作流来说完全没问题，而且你可以运行任意数量的 Coast，而完全不使用 `coast checkout`。

## When Checkout Is Useful

在某些情况下，动态端口并不够用，你需要规范端口:

- **硬编码为规范端口的客户端应用。** 如果你有一个运行在 Coast 外部的客户端——比如主机上的前端开发服务器、手机上的移动应用，或者桌面应用——它期望使用 `localhost:3000` 或 `localhost:8080`，那么到处修改端口号会很不现实。checkout 该 Coast 后，你就可以在不更改任何配置的情况下使用真实端口。

- **Webhook 和回调 URL。** 像 Stripe、GitHub 或 OAuth 提供商这样的服务会将回调发送到你已注册的 URL——通常类似于 `https://your-ngrok-tunnel.io`，并转发到 `localhost:3000`。如果你切换到动态端口，这些回调就不会再到达。checkout 可确保你正在测试的 Coast 使用的是活动的规范端口。

- **数据库工具、调试器和 IDE 集成。** 许多 GUI 客户端（pgAdmin、DataGrip、TablePlus）、调试器和 IDE 运行配置都会以特定端口保存连接配置。Checkout 让你可以保留这些已保存的配置，只需切换其背后的 Coast——无需在每次切换上下文时重新配置调试器附加目标或数据库连接。

## Releasing Checkout

如果你想释放规范端口，而不是 checkout 到另一个 Coast:

```bash
coast checkout --none
```

执行后，将不会有任何 Coast 拥有这些规范端口。所有 Coast 仍然可以通过它们各自的动态端口访问。

## Only One at a Time

任意时刻只能有一个 Coast 被 checkout。如果 `dev-1` 已被 checkout，而你运行 `coast checkout dev-2`，规范端口会立即切换到 `dev-2`。中间不会有空档——旧的转发器会被终止，新的转发器会在同一次操作中生成。

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

动态端口不受 checkout 影响。唯一发生变化的是规范端口所指向的位置。
