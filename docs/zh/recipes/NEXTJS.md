# Next.js 应用程序

本配方适用于由 Postgres 和 Redis 提供支持的 Next.js 应用程序，并可选配后台 worker 或配套服务。该技术栈将 Next.js 作为[裸服务](../concepts_and_terminology/BARE_SERVICES.md)运行，并使用 Turbopack 实现快速 HMR；同时，Postgres 和 Redis 作为宿主机上的[共享服务](../concepts_and_terminology/SHARED_SERVICES.md)运行，因此每个 Coast 实例共享相同的数据。

此模式在以下情况下效果很好:

- 你的项目在开发中使用带 Turbopack 的 Next.js
- 你的应用程序有数据库和缓存层（Postgres、Redis）作为支撑
- 你希望多个 Coast 实例并行运行，而无需为每个实例单独设置数据库
- 你使用像 NextAuth 这样的认证库，它们会在响应中嵌入回调 URL

## 完整的 Coastfile

```toml
[coast]
name = "my-nextjs-app"
primary_port = "web"
private_paths = ["packages/web/.next"]
worktree_dir = [".worktrees", ".claude/worktrees"]

[coast.setup]
packages = ["nodejs", "npm", "make", "git", "bash"]
run = [
    "npm install -g corepack",
    "corepack enable",
]

# --- Bare services: Next.js and background worker ---

[services.web]
install = [
    "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)",
    "cd /workspace && test -f config.json || echo {} > config.json",
    "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn prisma migrate dev",
]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev"
port = 3000
restart = "on-failure"
cache = ["node_modules"]

[services.worker]
install = "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)"
command = "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev:worker"
restart = "on-failure"
cache = ["node_modules"]

# --- Shared services: Postgres and Redis on the host ---

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["myapp_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_DB = "postgres", POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "postgres" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["myapp_redis_data:/data"]

# --- Secrets: connection strings for bare services ---

[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"

[secrets.redis_url]
extractor = "command"
run = "echo redis://host.docker.internal:6379"
inject = "env:REDIS_URL"

# --- Ports ---

[ports]
web = 3000
postgres = 5432
redis = 6379

# --- Assign: branch-switch behavior ---

[assign]
default = "none"
exclude_paths = ["docs", ".github", "scripts"]

[assign.services]
web = "hot"
worker = "hot"

[assign.rebuild_triggers]
web = ["package.json", "yarn.lock"]
worker = ["package.json", "yarn.lock"]
```

## 项目与设置

```toml
[coast]
name = "my-nextjs-app"
primary_port = "web"
private_paths = ["packages/web/.next"]
worktree_dir = [".worktrees", ".claude/worktrees"]
```

**`private_paths`** 对 Next.js 至关重要。Turbopack 在启动时会在 `.next/dev/lock` 创建一个锁文件。如果没有 `private_paths`，同一分支上的第二个 Coast 实例会看到这个锁并拒绝启动。使用它后，每个实例都会通过按实例划分的 overlay 挂载获得自己隔离的 `.next` 目录。参见 [Private Paths](../concepts_and_terminology/PRIVATE_PATHS.md)。

**`worktree_dir`** 列出 git worktree 所在的目录。如果你使用多个编码代理（Claude Code、Cursor、Codex），它们可能会在不同位置创建 worktree。把这些目录都列出来，可以让 Coast 发现并分配 worktree，而不受创建它们的工具影响。

```toml
[coast.setup]
packages = ["nodejs", "npm", "make", "git", "bash"]
run = [
    "npm install -g corepack",
    "corepack enable",
]
```

setup 部分安装裸服务所需的系统包和工具。`corepack enable` 会根据项目的 `packageManager` 字段启用 yarn 或 pnpm。这些命令在 Coast 镜像内部的构建时运行，而不是在实例启动时运行。

## 裸服务

```toml
[services.web]
install = [
    "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)",
    "cd /workspace && test -f config.json || echo {} > config.json",
    "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn prisma migrate dev",
]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev"
port = 3000
restart = "on-failure"
cache = ["node_modules"]
```

**条件安装:** `test -f node_modules/.yarn-state.yml || make yarn` 这种模式会在 `node_modules` 已存在时跳过依赖安装。当依赖没有变化时，这能让分支切换更快。参见 [Bare Service Optimization](../concepts_and_terminology/BARE_SERVICE_OPTIMIZATION.md)。

**`cache`:** 在 worktree 切换时保留 `node_modules`，这样 `yarn install` 就能增量运行，而不是每次从头开始。

**带动态端口的 `AUTH_URL`:** 使用 NextAuth（或类似认证库）的 Next.js 应用会在响应中嵌入回调 URL。在 Coast 内部，Next.js 监听 3000 端口，但宿主机侧端口是动态的。Coast 会自动将 `WEB_DYNAMIC_PORT` 注入到容器环境中（从 `[ports]` 中的 `web` 键推导而来）。`:-3000` 回退值意味着同一条命令在 Coast 外部也能工作。参见 [Dynamic Port Environment Variables](../concepts_and_terminology/DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md)。

**`host.docker.internal`:** 裸服务无法通过 `localhost` 访问共享服务，因为共享服务运行在宿主机 Docker daemon 上。`host.docker.internal` 会在 Coast 容器内部解析到宿主机。

## 共享服务

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["myapp_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_DB = "postgres", POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "postgres" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["myapp_redis_data:/data"]
```

Postgres 和 Redis 作为[共享服务](../concepts_and_terminology/SHARED_SERVICES.md)运行在宿主机 Docker daemon 上。每个 Coast 实例都连接到同一组数据库，因此用户、会话和数据会在各实例之间共享。这样可以避免必须在每个实例中分别注册的麻烦。

如果你的项目已经有一个包含 Postgres 和 Redis 的 `docker-compose.yml`，你也可以改用 `compose`，并将 volume strategy 设置为 `shared`。对于裸服务 Coastfile 来说，共享服务更简单，因为无需管理 compose 文件。

## Secrets

```toml
[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"

[secrets.redis_url]
extractor = "command"
run = "echo redis://host.docker.internal:6379"
inject = "env:REDIS_URL"
```

这些配置会在构建时将 `DATABASE_URL` 和 `REDIS_URL` 注入到 Coast 容器环境中。连接字符串通过 `host.docker.internal` 指向共享服务。

`command` 提取器会运行一个 shell 命令并捕获标准输出。这里它只是输出一个静态字符串，但你也可以用它从 vault 读取、运行 CLI 工具，或动态计算某个值。

请注意，裸服务的 `command` 字段也会以内联方式设置这些变量。内联值优先级更高，但注入的 secrets 可作为 `install` 步骤和 `coast exec` 会话的默认值。

## Assign 策略

```toml
[assign]
default = "none"
exclude_paths = ["docs", ".github", "scripts"]

[assign.services]
web = "hot"
worker = "hot"

[assign.rebuild_triggers]
web = ["package.json", "yarn.lock"]
worker = ["package.json", "yarn.lock"]
```

**`default = "none"`** 会在分支切换时保持共享服务和基础设施不变。只有依赖代码的服务才需要 assign 策略。

**为 Next.js 和 worker 使用 `hot`:** 带 Turbopack 的 Next.js 内置热模块替换。当 Coast 将 `/workspace` 重新挂载到新的 worktree 时，Turbopack 会检测到文件变化并自动重新编译。无需重启进程。使用 `tsc --watch` 或 `nodemon` 的后台 worker 也会通过它们的文件监听器捕捉到变化。

**`rebuild_triggers`:** 如果在分支之间 `package.json` 或 `yarn.lock` 发生了变化，服务的 `install` 命令会在服务重启前重新运行。这能确保在切换到添加或删除了包的分支后，依赖仍保持最新。

**`exclude_paths`:** 通过跳过服务不需要的目录，加快首次 worktree 引导。文档、CI 配置和脚本都可以安全地排除。

## 调整此配方

**没有后台 worker:** 删除 `[services.worker]` 部分及其 assign 条目。Coastfile 的其余部分无需更改即可工作。

**包含多个 Next.js 应用的 monorepo:** 为每个应用的 `.next` 目录添加一条 `private_paths`。每个裸服务都应有各自的 `[services.*]` 部分，并配置相应的 `command` 和 `port`。

**使用 pnpm 而不是 yarn:** 将 `make yarn` 替换为你的 pnpm 安装命令。如果 pnpm 将依赖存储在不同位置（例如 `.pnpm-store`），请相应调整 `cache` 字段。

**没有共享服务:** 如果你更喜欢按实例划分的数据库，删除 `[shared_services]` 和 `[secrets]` 部分。将 Postgres 和 Redis 添加到一个 `docker-compose.yml` 中，在 `[coast]` 部分设置 `compose`，并使用 [volume strategies](../coastfiles/VOLUMES.md) 控制隔离。对按实例划分的数据使用 `strategy = "isolated"`，对共享数据使用 `strategy = "shared"`。

**额外的认证提供方:** 如果你的认证库对回调 URL 使用的环境变量不是 `AUTH_URL`，请在服务命令中的这些变量上应用同样的 `${WEB_DYNAMIC_PORT:-3000}` 模式。
