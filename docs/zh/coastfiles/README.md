# Coastfiles

Coastfile 是一个 TOML 配置文件，位于你项目的根目录。它告诉 Coast 构建并运行该项目的隔离开发环境所需的一切信息——要运行哪些服务、要转发哪些端口、如何处理数据，以及如何管理机密。

每个 Coast 项目至少需要一个 Coastfile。该文件始终命名为 `Coastfile`（大写 C，无扩展名）。如果你需要用于不同工作流的变体，可以创建带类型的 Coastfile，例如 `Coastfile.light` 或 `Coastfile.snap`，它们会[继承自基础文件](INHERITANCE.md)。

要更深入理解 Coastfile 与 Coast 其余部分的关系，请参阅 [Coasts](../concepts_and_terminology/COASTS.md) 和 [Builds](../concepts_and_terminology/BUILDS.md)。

## Quickstart

最小可用的 Coastfile:

```toml
[coast]
name = "my-app"
```

这会给你一个 DinD 容器，你可以 `coast exec` 进入其中。大多数项目会需要一个 `compose` 引用或[裸服务](SERVICES.md):

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
web = 3000
api = 8080
```

或者不使用 compose，改用裸服务:

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

运行 `coast build` 然后 `coast run dev-1`，你就拥有了一个隔离环境。

## Example Coastfiles

### Simple bare-service project

一个没有 compose 文件的 Next.js 应用。Coast 安装 Node，运行 `npm install`，并直接启动开发服务器。

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

### Full-stack compose project

一个多服务项目，包含共享数据库、机密、卷策略以及自定义设置。

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
worktree_dir = ".worktrees"
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

### Lightweight test variant (inheritance)

扩展基础 Coastfile，但将其精简到仅包含运行后端测试所需内容。无端口、无共享服务、隔离数据库。

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

### Snapshot-seeded variant

每个 coast 实例都会以宿主机现有数据库卷的副本启动，然后各自独立分叉演进。

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

## Conventions

- 文件必须命名为 `Coastfile`（大写 C，无扩展名），并位于项目根目录。
- 带类型的变体使用 `Coastfile.{type}` 模式——例如 `Coastfile.light`、`Coastfile.snap`。参见 [Inheritance and Types](INHERITANCE.md)。
- 保留名称 `Coastfile.default` 不允许使用。
- 全程使用 TOML 语法。所有小节标题使用 `[brackets]`，命名条目使用 `[section.name]`（不是 array-of-tables）。
- 你不能在同一个 Coastfile 中同时使用 `compose` 和 `[services]`——二选一。
- 相对路径（用于 `compose`、`root` 等）会相对于 Coastfile 所在目录解析。

## Reference

| Page | Sections | What it covers |
|------|----------|----------------|
| [Project and Setup](PROJECT.md) | `[coast]`, `[coast.setup]` | 名称、compose 路径、运行时、worktree 目录、容器设置 |
| [Ports](PORTS.md) | `[ports]`, `[egress]` | 端口转发、egress 声明、主端口 |
| [Volumes](VOLUMES.md) | `[volumes.*]` | 隔离、共享与快照种子卷策略 |
| [Shared Services](SHARED_SERVICES.md) | `[shared_services.*]` | 宿主级数据库与基础设施服务 |
| [Secrets](SECRETS.md) | `[secrets.*]`, `[inject]` | 机密提取、注入与宿主环境/文件转发 |
| [Bare Services](SERVICES.md) | `[services.*]` | 不使用 Docker Compose 直接运行进程 |
| [Agent Shell](AGENT_SHELL.md) | `[agent_shell]` | 容器化 agent TUI 运行时 |
| [MCP Servers](MCP.md) | `[mcp.*]`, `[mcp_clients.*]` | 内部与宿主代理的 MCP 服务器、客户端连接器 |
| [Assign](ASSIGN.md) | `[assign]` | 按服务的分支切换行为 |
| [Inheritance and Types](INHERITANCE.md) | `extends`, `includes`, `[unset]`, `[omit]` | 带类型的 Coastfile、组合与覆盖 |
