# MCP 服务器

MCP（Model Context Protocol，模型上下文协议）服务器为 AI 代理提供对工具的访问——文件搜索、数据库查询、文档查找、浏览器自动化等。Coast 可以在 Coast 容器内安装和配置 MCP 服务器，使容器化的代理能够访问其所需的工具。

**这仅在你在 Coast 容器内运行代理时才相关。** 如果你在主机上运行代理（推荐方式），你的 MCP 服务器也会在主机上运行，因此不需要任何此类配置。本页面基于 [Agent Shells](AGENT_SHELLS.md)，并在其之上增加了一层复杂性。在继续之前，请先阅读那里的警告。

## 内部服务器 vs 主机代理服务器

Coast 支持两种 MCP 服务器模式，由 Coastfile 的 `[mcp]` 段中的 `proxy` 字段控制。

### 内部服务器

内部服务器会安装并运行在 DinD 容器内的 `/mcp/<name>/` 下。它们可以直接访问容器化的文件系统以及正在运行的服务。

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
```

你也可以将项目中的源文件复制到 MCP 目录中:

```toml
[mcp.my-custom-tool]
source = "tools/my-mcp-server"
install = ["npm install", "npm run build"]
command = "node"
args = ["dist/index.js"]
```

`source` 字段会在设置期间将文件从 `/workspace/<path>/` 复制到 `/mcp/<name>/`。`install` 命令会在该目录内运行。这对于位于你的代码仓库中的 MCP 服务器很有用。

### 主机代理服务器

主机代理服务器运行在你的主机上，而不是容器内。Coast 会生成一个客户端配置，使用 `coast-mcp-proxy` 通过网络将 MCP 请求从容器转发到主机。

```toml
[mcp.browser]
proxy = "host"
command = "npx"
args = ["@anthropic-ai/browser-mcp"]
```

主机代理服务器不能包含 `install` 或 `source` 字段——它们应当已经在主机上可用。对于需要主机级访问的 MCP 服务器（例如浏览器自动化或主机文件系统工具），使用此模式。

### 何时使用哪一种

| 模式 | 运行位置 | 适用场景 | 限制 |
|---|---|---|---|
| 内部 | DinD 容器 | 需要访问容器文件系统的工具、项目特定工具 | 必须可在 Alpine Linux 上安装，会增加 `coast run` 时间 |
| 主机代理 | 主机 | 浏览器自动化、主机级工具、体积很大且已预装的服务器 | 无法直接访问容器文件系统 |

## 客户端连接器

`[mcp_clients]` 段告诉 Coast 将生成的 MCP 服务器配置写到哪里，以便容器内的代理能够发现这些服务器。

### 内置格式

对于 Claude Code 和 Cursor，只需要一个具有正确名称的空段即可——Coast 会自动检测格式和默认配置路径:

```toml
[mcp_clients.claude-code]
# Writes to /root/.claude/mcp_servers.json (auto-detected)

[mcp_clients.cursor]
# Writes to /workspace/.cursor/mcp.json (auto-detected)
```

### 自定义配置路径

对于其他 AI 工具，请显式指定格式与路径:

```toml
[mcp_clients.my-tool]
format = "claude-code"
config_path = "/home/coast/.config/my-tool/mcp.json"
```

### 基于命令的连接器

除了写入文件，你还可以将生成的配置 JSON 通过管道传给一个命令:

```toml
[mcp_clients.custom-setup]
run = "my-config-tool import-mcp --stdin"
```

`run` 字段与 `format` 和 `config_path` 互斥。

## Coastguard MCP 选项卡

[Coastguard](COASTGUARD.md) Web UI 在 MCP 选项卡中提供对你的 MCP 配置的可视化能力。

![MCP tab in Coastguard](../../assets/coastguard-mcp.png)
*Coastguard 的 MCP 选项卡，显示已配置的服务器、它们的工具以及客户端配置位置。*

该选项卡有三个部分:

- **MCP Servers** — 列出每个声明的服务器，包括其名称、类型（Internal 或 Host）、命令以及状态（Installed、Proxied 或 Not Installed）。
- **Tools** — 选择一个服务器以检查其通过 MCP 协议暴露的工具。每个工具显示其名称和描述；点击可查看完整的输入 schema。
- **Client Locations** — 显示生成的配置文件被写入的位置（例如:`claude-code` 格式写入 `/root/.claude/mcp_servers.json`）。

## CLI 命令

```bash
coast mcp dev-1 ls                          # list servers with type and status
coast mcp dev-1 tools context7              # list tools exposed by a server
coast mcp dev-1 tools context7 info resolve # show input schema for a specific tool
coast mcp dev-1 locations                   # show where client configs were written
```

`tools` 命令通过向容器内的 MCP 服务器进程发送 JSON-RPC `initialize` 与 `tools/list` 请求来工作。它仅适用于内部服务器——主机代理服务器必须在主机侧进行检查。

## 安装如何工作

在 `coast run` 期间，当内部 Docker 守护进程就绪且服务正在启动后，Coast 会设置 MCP:

1. 对于每个 **内部** MCP 服务器:
   - 在 DinD 容器内创建 `/mcp/<name>/`
   - 如果设置了 `source`，将文件从 `/workspace/<source>/` 复制到 `/mcp/<name>/`
   - 在 `/mcp/<name>/` 内运行每条 `install` 命令（例如:`npm install -g @upstash/context7-mcp`）

2. 对于每个 **客户端连接器**:
   - 以合适的格式生成 JSON 配置（Claude Code 或 Cursor）
   - 内部服务器使用其实际的 `command` 与 `args`，并将 `cwd` 设置为 `/mcp/<name>/`
   - 主机代理服务器将命令设置为 `coast-mcp-proxy`，并将服务器名称作为参数
   - 将配置写入目标路径（或通过管道传给 `run` 命令）

主机代理服务器依赖容器内的 `coast-mcp-proxy` 将 MCP 协议请求转发回主机，在主机上运行实际的 MCP 服务器进程。

## 完整示例

一个 Coastfile:设置一个内部文档工具和一个主机代理的浏览器工具，并接入 Claude Code:

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp.browser]
proxy = "host"
command = "npx"
args = ["@anthropic-ai/browser-mcp"]

[mcp_clients.claude-code]
```

在 `coast run` 之后，容器内的 Claude Code 会在其 MCP 配置中看到两个服务器——`context7` 在本地的 `/mcp/context7/` 运行，而 `browser` 通过代理连接到主机。

## 在主机上运行的代理

如果你的编码代理运行在主机上（推荐方式），你的 MCP 服务器也运行在主机上，且不会涉及 Coast 的 `[mcp]` 配置。不过，有一件事需要考虑:**连接到 Coast 内部数据库或服务的 MCP 服务器需要知道正确的端口。**

当服务运行在 Coast 内时，它们可通过动态端口访问，并且每次运行新实例时端口都会变化。主机上的数据库 MCP 若连接到 `localhost:5432`，只会连接到被 [checked-out](CHECKOUT.md) 的 Coast 数据库——如果没有任何 Coast 被 checked out，则可能什么也连接不到。对于未 checked-out 的实例，你需要重新配置 MCP 使用 [dynamic port](PORTS.md)（例如:`localhost:55681`）。

有两种办法可以解决:

**使用共享服务。** 如果你的数据库作为 [shared service](SHARED_SERVICES.md) 运行，它位于主机 Docker 守护进程上并使用其规范端口（`localhost:5432`）。每个 Coast 实例都通过桥接网络连接到它，而你的主机侧 MCP 也连接到同一个数据库，并始终使用相同端口。无需重新配置，也无需发现动态端口。这是最简单的方法。

**使用 `coast exec` 或 `coast docker`。** 如果你的数据库运行在 Coast 内（隔离卷），你的主机侧代理仍然可以通过 Coast 运行命令来查询它（参见 [Exec & Docker](EXEC_AND_DOCKER.md)）:

```bash
coast exec dev-1 -- psql -h localhost -U myuser -d mydb -c "SELECT count(*) FROM users"
coast docker dev-1 exec -i my-postgres psql -U myuser -d mydb -c "\\dt"
```

这避免了必须知道动态端口——命令在 Coast 内运行，而数据库在那里的规范端口上可用。

对于大多数工作流，共享服务是阻力最小的路径。你的主机 MCP 配置会与开始使用 Coasts 之前完全一致。
