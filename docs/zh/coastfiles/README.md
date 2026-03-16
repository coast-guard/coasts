# Coastfile

Coastfile 是一个 TOML 配置文件，位于你的项目根目录。它会告诉 Coast 构建和运行该项目的隔离开发环境所需了解的一切——运行哪些服务、转发哪些端口、如何处理数据，以及如何管理密钥。

每个 Coast 项目都至少需要一个 Coastfile。该文件的名称始终为 `Coastfile`（大写 C，无扩展名）。如果你需要针对不同工作流的变体，可以创建带类型的 Coastfile，例如 `Coastfile.light` 或 `Coastfile.snap`，它们会[继承基础配置](INHERITANCE.md)。

若要更深入地理解 Coastfile 与 Coast 其他部分之间的关系，请参阅 [Coasts](../concepts_and_terminology/COASTS.md) 和 [Builds](../concepts_and_terminology/BUILDS.md)。

## 快速开始

最小可用的 Coastfile:

```toml
[coast]
name = "my-app"
```

这会为你提供一个可以通过 `coast exec` 进入的 DinD 容器。大多数项目通常会需要一个 `compose` 引用或[裸服务](SERVICES.md):

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
web = 3000
api = 8080
```

或者不使用 compose，而是使用裸服务:

```toml
[coast]
name = "my-app"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
restart = "on-failure"

[ports]
web = 3000
```

运行 `coast build`，然后运行 `coast run dev-1`，你就拥有了一个隔离环境。

## Coastfile 示例

### 简单的裸服务项目

一个没有 compose 文件的 Next.js 应用。Coast 会安装 Node，运行 `npm install`，并直接启动开发服务器。

```toml
[coast]
name = "my-crm"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --turbopack --port 3002 --hostname 0.0.0.0"
port = 3002
restart = "on-failure"

[ports]
web = 3002
```

### 全栈 compose 项目

一个多服务项目，包含共享数据库、密钥、卷策略以及自定义设置。

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
primary_port = "web"

[coast.setup]
packages = ["nodejs", "npm", "python3", "curl", "git", "bash", "ca-certificates", "wget"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]

[ports]
web = 3000
backend = 8080
postgres = 5432
redis = 6379

[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass" }

[shared_services.redis]
image = "redis:7"
ports = [6379]

[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"

[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"

[omit]
services = ["monitoring", "admin-panel", "nginx-proxy"]

[assign]
default = "none"
[assign.services]
backend = "hot"
web = "hot"
```

### 轻量级测试变体（继承）

扩展基础 Coastfile，但将其精简为仅保留运行后端测试所需的内容。没有端口，没有共享服务，数据库隔离。

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "backend", "postgres", "redis"]
shared_services = ["postgres", "redis"]

[omit]
services = ["redis", "backend", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[assign]
default = "none"
[assign.services]
backend-test = "rebuild"
```

### 快照播种变体

每个 coast 实例启动时都会复制主机上现有数据库卷的内容，然后各自独立分化。

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis", "mongodb"]

[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "infra_redis_data"
service = "redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
snapshot_source = "infra_mongodb_data"
service = "mongodb"
mount = "/data/db"
```

## 约定

- 文件必须命名为 `Coastfile`（大写 C，无扩展名），并位于项目根目录。
- 带类型的变体使用 `Coastfile.{type}` 模式——例如 `Coastfile.light`、`Coastfile.snap`。参见 [继承与类型](INHERITANCE.md)。
- 保留名称 `Coastfile.default` 不允许使用。
- 全文使用 TOML 语法。所有节标题都使用 `[brackets]`，命名条目使用 `[section.name]`（不是 array-of-tables）。
- 你不能在同一个 Coastfile 中同时使用 `compose` 和 `[services]`——二选一。
- 相对路径（用于 `compose`、`root` 等）会相对于 Coastfile 的父目录进行解析。

## 参考

| 页面 | 节 | 覆盖内容 |
|------|----------|----------------|
| [项目与设置](PROJECT.md) | `[coast]`, `[coast.setup]` | 名称、compose 路径、运行时、worktree 目录、容器设置 |
| [Worktree 目录](WORKTREE_DIR.md) | `worktree_dir`, `default_worktree_dir` | 本地和外部 worktree 目录、波浪线路径、Codex/Claude 集成 |
| [端口](PORTS.md) | `[ports]`, `[egress]` | 端口转发、出口声明、主端口 |
| [卷](VOLUMES.md) | `[volumes.*]` | 隔离、共享和快照播种的卷策略 |
| [共享服务](SHARED_SERVICES.md) | `[shared_services.*]` | 主机级数据库和基础设施服务 |
| [密钥](SECRETS.md) | `[secrets.*]`, `[inject]` | 密钥提取、注入，以及主机环境/文件转发 |
| [裸服务](SERVICES.md) | `[services.*]` | 无需 Docker Compose 直接运行进程 |
| [代理 Shell](AGENT_SHELL.md) | `[agent_shell]` | 容器化代理 TUI 运行时 |
| [MCP 服务器](MCP.md) | `[mcp.*]`, `[mcp_clients.*]` | 内部和主机代理的 MCP 服务器、客户端连接器 |
| [Assign](ASSIGN.md) | `[assign]` | 按服务划分的分支切换行为 |
| [继承与类型](INHERITANCE.md) | `extends`, `includes`, `[unset]`, `[omit]` | 带类型的 Coastfile、组合与覆盖 |
