# MCP 服务器与客户端

> **注意:** 只有当你通过 [`[agent_shell]`](AGENT_SHELL.md) 在 Coast 容器内运行编码代理时，MCP 配置才相关。如果你的代理运行在主机上（更常见的设置），它已经可以访问自己的 MCP 服务器，不需要 Coast 来配置它们。

`[mcp.*]` 部分用于配置在你的 Coast 实例内部或旁边运行的 MCP（Model Context Protocol，模型上下文协议）服务器。`[mcp_clients.*]` 部分会将这些服务器接入到 Claude Code 或 Cursor 等编码代理中，使它们能够自动发现并使用这些服务器。

关于 MCP 服务器在运行时如何安装、代理与管理，请参阅 [MCP Servers](../concepts_and_terminology/MCP_SERVERS.md)。

## MCP 服务器 — `[mcp.*]`

每个 MCP 服务器都是 `[mcp]` 下的一个具名 TOML section。共有两种模式:**内部**（在 Coast 容器内运行）与 **主机代理**（在主机上运行，通过代理接入 Coast）。

### 内部 MCP 服务器

内部服务器会在 DinD 容器内安装并运行。当没有 `proxy` 时，`command` 字段是必需的。

```toml
[mcp.echo]
command = "node"
args = ["server.js"]
```

字段:

- **`command`**（必需）— 要运行的可执行文件
- **`args`** — 传递给命令的参数
- **`env`** — 服务器进程的环境变量
- **`install`** — 启动服务器前要运行的命令（接受字符串或数组）
- **`source`** — 要复制到容器中 `/mcp/{name}/` 的主机目录

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]
```

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
```

### 主机代理的 MCP 服务器

主机代理服务器运行在你的主机上，并通过 `coast-mcp-proxy` 在 Coast 内可用。设置 `proxy = "host"` 以启用此模式。

```toml
[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]
env = { MCP_MODE = "host" }
```

当 `proxy = "host"` 时:

- `command`、`args` 和 `env` 是可选的——如果省略，则会按名称从主机现有的 MCP 配置中解析该服务器。
- `install` 与 `source` **不允许**使用（服务器运行在主机上，而不是容器内）。

一个不含额外字段的主机代理服务器会按名称从你的主机配置中查找该服务器:

```toml
[mcp.host-lookup]
proxy = "host"
```

`proxy` 唯一有效的值是 `"host"`。

### 多个服务器

你可以定义任意数量的 MCP 服务器:

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]

[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]

[mcp.host-lookup]
proxy = "host"
```

## MCP 客户端 — `[mcp_clients.*]`

MCP 客户端连接器会告诉 Coast 如何将 MCP 服务器配置写入编码代理所读取的配置文件中。这会自动把你的 `[mcp.*]` 服务器接入到代理中。

### 内置连接器

内置了两个连接器:`claude-code` 和 `cursor`。使用它们不需要任何额外字段。

```toml
[mcp_clients.claude-code]
```

```toml
[mcp_clients.cursor]
```

内置连接器会自动知道:

- **`claude-code`** — 写入 `/root/.claude/mcp_servers.json`
- **`cursor`** — 写入 `/workspace/.cursor/mcp.json`

你可以覆盖配置路径:

```toml
[mcp_clients.claude-code]
config_path = "/custom/path/mcp_servers.json"
```

### 自定义连接器

对于非内置的代理，使用 `run` 字段指定一个 shell 命令，Coast 会执行该命令来注册 MCP 服务器:

```toml
[mcp_clients.my-agent]
run = "my-agent mcp register --stdin"
```

`run` 字段不能与 `format` 或 `config_path` 组合使用。

### 自定义格式连接器

如果你的代理使用与 Claude Code 或 Cursor 相同的配置文件格式，但位于不同路径:

```toml
[mcp_clients.my-agent]
format = "claude-code"
config_path = "/home/agent/.config/mcp.json"
```

`format` 必须是 `"claude-code"` 或 `"cursor"`。当使用带有 `format` 的非内置名称时，`config_path` 是必需的。

## 示例

### 接入 Claude Code 的内部 MCP 服务器

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp_clients.claude-code]
```

### 带有内部服务器的主机代理服务器

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]

[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]
env = { MCP_MODE = "host" }

[mcp_clients.claude-code]
```

### 多个客户端连接器

```toml
[mcp.my-tools]
command = "my-mcp-server"
args = ["--port", "3100"]

[mcp_clients.claude-code]
[mcp_clients.cursor]
```
