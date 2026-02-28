# 日志

Coast 内部的服务运行在嵌套容器中——你的 compose 服务由一个 DinD 容器里的内部 Docker 守护进程管理。这意味着主机级的日志工具无法看到它们。如果你的工作流包含一个读取主机上 Docker 日志的日志 MCP，那么它只能看到外层的 DinD 容器，而看不到在其内部运行的 Web 服务器、数据库或 worker。

解决方案是 `coast logs`。任何需要从 Coast 实例读取服务输出的 agent 或工具，都必须使用 Coast CLI，而不是主机级的 Docker 日志访问。

## MCP 的取舍

如果你正在使用带有日志 MCP 的 AI agent（一种从你的主机捕获 Docker 容器日志的工具——见 [MCP Servers](MCP_SERVERS.md)），那么该 MCP 对运行在 Coast 内部的服务将不起作用。主机 Docker 守护进程对每个 Coast 实例只看到一个容器——DinD 容器——其日志只是内部 Docker 守护进程的启动输出。

要捕获真实的服务日志，请指示你的 agent 使用:

```bash
coast logs <instance> --service <service> --tail <lines>
```

例如，如果你的 agent 需要检查某个后端服务为何失败:

```bash
coast logs dev-1 --service backend --tail 100
```

这等价于 `docker compose logs`，但会通过 Coast 守护进程路由到内部的 DinD 容器中。如果你有引用日志 MCP 的 agent 规则或系统提示词，那么在 Coast 内工作时，你需要添加一条指令来覆盖这种行为。

## `coast logs`

CLI 提供了多种方式来读取 Coast 实例的日志:

```bash
coast logs dev-1                           # last 200 lines, all services
coast logs dev-1 --service web             # last 200 lines, web only
coast logs dev-1 --tail 50                 # last 50 lines, then follow
coast logs dev-1 --tail                    # all lines, then follow
coast logs dev-1 --service backend -f      # follow mode (stream new entries)
coast logs dev-1 --service web --tail 100  # last 100 lines + follow
```

在不使用 `--tail` 或 `-f` 的情况下，该命令返回最后 200 行并退出。使用 `--tail` 时，它会流式输出请求数量的行，然后继续实时跟随新的输出。`-f` / `--follow` 则单独启用跟随模式。

输出使用 compose 的日志格式，每行带有服务前缀:

```text
web       | 2026/02/28 01:49:34 Listening on :3000
backend   | 2026/02/28 01:49:34 [INFO] Server started on :8080
backend   | 2026/02/28 01:49:34 [ProcessCreditsJob] starting at 2026-02-28T01:49:34Z
redis     | 1:M 28 Feb 2026 01:49:30.123 * Ready to accept connections
```

你也可以使用旧的按位置语法按服务过滤（`coast logs dev-1 web`），但更推荐使用 `--service` 标志。

## Coastguard 日志选项卡

Coastguard Web UI 通过 WebSocket 提供更丰富的日志查看体验，并支持实时流式输出。

![Logs tab in Coastguard](../../assets/coastguard-logs.png)
*Coastguard 日志选项卡正在流式显示后端服务输出，并支持服务过滤与搜索。*

日志选项卡提供:

- **实时流** — 日志通过 WebSocket 连接在产生时即到达，并带有状态指示器显示连接状态。
- **服务过滤器** — 下拉列表由日志流的服务前缀填充。选择单个服务以聚焦其输出。
- **搜索** — 按文本或正则过滤显示的行（切换星号按钮以启用正则模式）。匹配项会被高亮。
- **行数统计** — 显示过滤后的行数与总行数（例如 “200 / 971 lines”）。
- **清空** — 截断内部容器日志文件并重置查看器。
- **全屏** — 将日志查看器扩展为全屏。

日志行支持 ANSI 颜色渲染、日志级别高亮（ERROR 为红色、WARN 为琥珀色、INFO 为蓝色、DEBUG 为灰色）、时间戳淡化，以及用于区分服务的彩色服务徽标。

在主机守护进程上运行的共享服务有其独立的日志查看器，可从 Shared Services 选项卡访问。详情见 [Shared Services](SHARED_SERVICES.md)。

## 工作原理

当你运行 `coast logs` 时，守护进程会通过 `docker exec` 在 DinD 容器内执行 `docker compose logs`，并将输出流式返回到你的终端（或通过 WebSocket 返回到 Coastguard UI）。

```text
coast logs dev-1 --service web --tail 50
  │
  ├── CLI sends LogsRequest to daemon (Unix socket)
  │
  ├── Daemon resolves instance → container ID
  │
  ├── Daemon exec's into DinD container:
  │     docker compose logs --tail 50 --follow web
  │
  └── Output streams back chunk by chunk
        └── CLI prints to stdout / Coastguard renders in UI
```

对于 [bare services](BARE_SERVICES.md)，守护进程会 tail `/var/log/coast-services/` 下的日志文件，而不是调用 `docker compose logs`。输出格式相同（`service  | line`），因此在两种情况下服务过滤的工作方式完全一致。

## 相关命令

- `coast ps <instance>` — 检查哪些服务正在运行及其状态。参见 [Runtimes and Services](RUNTIMES_AND_SERVICES.md)。
- [`coast exec <instance>`](EXEC_AND_DOCKER.md) — 在 Coast 容器内打开一个 shell 以进行手动调试。
