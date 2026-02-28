# MCP Servers and Clients

> **Note:** MCP configuration is only relevant when you are running a coding agent inside a Coast container via [`[agent_shell]`](AGENT_SHELL.md). If your agent runs on the host (the more common setup), it already has access to its own MCP servers and does not need Coast to configure them.

The `[mcp.*]` sections configure MCP (Model Context Protocol) servers that run inside or alongside your Coast instances. The `[mcp_clients.*]` sections wire those servers into coding agents like Claude Code or Cursor so they can discover and use them automatically.

For how MCP servers are installed, proxied, and managed at runtime, see [MCP Servers](../concepts_and_terminology/MCP_SERVERS.md).

## MCP Servers — `[mcp.*]`

Each MCP server is a named TOML section under `[mcp]`. There are two modes: **internal** (runs inside the Coast container) and **host-proxied** (runs on the host, proxied into the Coast).

### Internal MCP servers

An internal server is installed and runs inside the DinD container. The `command` field is required when there's no `proxy`.

```toml
[mcp.echo]
command = "node"
args = ["server.js"]
```

Fields:

- **`command`** (required) — the executable to run
- **`args`** — arguments passed to the command
- **`env`** — environment variables for the server process
- **`install`** — commands to run before starting the server (accepts a string or array)
- **`source`** — a host directory to copy into the container at `/mcp/{name}/`

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

### Host-proxied MCP servers

A host-proxied server runs on your host machine and is made available inside the Coast via `coast-mcp-proxy`. Set `proxy = "host"` to enable this mode.

```toml
[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]
env = { MCP_MODE = "host" }
```

When `proxy = "host"`:

- `command`, `args`, and `env` are optional — if omitted, the server is resolved from the host's existing MCP configuration by name.
- `install` and `source` are **not allowed** (the server runs on the host, not in the container).

A host-proxied server with no additional fields looks up the server by name from your host config:

```toml
[mcp.host-lookup]
proxy = "host"
```

The only valid value for `proxy` is `"host"`.

### Multiple servers

You can define any number of MCP servers:

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

## MCP Clients — `[mcp_clients.*]`

MCP client connectors tell Coast how to write MCP server configuration into the config files that coding agents read. This wires your `[mcp.*]` servers into agents automatically.

### Built-in connectors

Two connectors are built in: `claude-code` and `cursor`. Using them requires no additional fields.

```toml
[mcp_clients.claude-code]
```

```toml
[mcp_clients.cursor]
```

Built-in connectors automatically know:

- **`claude-code`** — writes to `/root/.claude/mcp_servers.json`
- **`cursor`** — writes to `/workspace/.cursor/mcp.json`

You can override the config path:

```toml
[mcp_clients.claude-code]
config_path = "/custom/path/mcp_servers.json"
```

### Custom connectors

For agents that aren't built in, use the `run` field to specify a shell command that Coast executes to register MCP servers:

```toml
[mcp_clients.my-agent]
run = "my-agent mcp register --stdin"
```

The `run` field cannot be combined with `format` or `config_path`.

### Custom format connectors

If your agent uses the same config file format as Claude Code or Cursor but lives at a different path:

```toml
[mcp_clients.my-agent]
format = "claude-code"
config_path = "/home/agent/.config/mcp.json"
```

The `format` must be `"claude-code"` or `"cursor"`. When using a non-built-in name with `format`, `config_path` is required.

## Examples

### Internal MCP server wired to Claude Code

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp_clients.claude-code]
```

### Host-proxied server with internal server

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

### Multiple client connectors

```toml
[mcp.my-tools]
command = "my-mcp-server"
args = ["--port", "3100"]

[mcp_clients.claude-code]
[mcp_clients.cursor]
```
