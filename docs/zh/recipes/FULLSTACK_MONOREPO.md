# 全栈 Monorepo

本配方适用于一个大型 monorepo:包含多个 Web 应用，这些应用由共享的数据库与缓存层支撑。该技术栈使用 Docker Compose 来运行重量级后端服务（Rails、Sidekiq、SSR），并在 DinD 宿主机上以裸服务的形式运行 Vite 开发服务器。Postgres 与 Redis 作为共享服务运行在宿主机 Docker daemon 上，这样每个 Coast 实例都连接到同一套基础设施，而无需重复启动它。

这种模式在以下场景效果很好:

- 你的 monorepo 包含多个共享同一数据库的应用
- 你希望 Coast 实例轻量化，不要让每个实例各自运行自己的 Postgres 和 Redis
- 你的前端开发服务器需要能从 compose 容器内通过 `host.docker.internal` 访问
- 你在宿主机侧有连接到 `localhost:5432` 的 MCP 集成，并希望它们保持不变继续工作

## 完整的 Coastfile

下面是完整的 Coastfile。后续将对每个部分进行详细说明。

```toml
[coast]
name = "acme"
compose = "./infra/docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "bash"]
run = ["npm install -g yarn"]

[ports]
api = 3000
web = 3002
vite-web = 3040
vite-api = 3036

[healthcheck]
web = "/"
api = "/"

# --- Bare services: Vite dev servers on the DinD host ---

[services.vite-web]
install = "cd /workspace && yarn install --immutable 2>/dev/null || yarn install"
command = "cd /workspace && yarn workspace @acme/web run dev"
port = 3040
restart = "on-failure"
cache = ["node_modules"]

[services.vite-api]
command = "cd /workspace && yarn workspace @acme/api run dev"
port = 3036
restart = "on-failure"

# --- Shared services: Postgres and Redis on the host daemon ---

[shared_services.db]
image = "postgres:15.3-alpine"
ports = [5432]
volumes = ["infra_postgres:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "password" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis:/data"]

# --- Volumes: shared caches across all instances ---

[volumes.bundle]
strategy = "shared"
service = "api-rails"
mount = "/usr/local/bundle"

[volumes.api_rails_cache]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/tmp/cache"

[volumes.api_assets]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/public/assets"

[volumes.web_rails_cache]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/tmp/cache"

[volumes.web_assets]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/public/assets"

# --- Secrets and injection ---

[secrets.compose_profiles]
extractor = "command"
run = "echo api,web"
inject = "env:COMPOSE_PROFILES"

[secrets.uid]
extractor = "command"
run = "id -u"
inject = "env:UID"

[secrets.gid]
extractor = "command"
run = "id -g"
inject = "env:GID"

[inject]
env = ["USER", "BUNDLE_GEMS__CONTRIBSYS__COM"]

# --- Assign: branch-switch behavior ---

[assign]
default = "none"
exclude_paths = [
    ".coasts",
    ".yarn",
    ".github",
    "docs",
    "scripts",
    "cli",
    "deploy",
    "tools",
    "mobile",
    "extensions",
]

[assign.services]
web-rails = "hot"
web-ssr = "hot"
web-sidekiq = "restart"
api-rails = "hot"
api-sidekiq = "restart"

[assign.rebuild_triggers]
web-rails = ["infra/Dockerfile", "web/Gemfile", "web/Gemfile.lock", "web/package.json"]
api-rails = ["infra/Dockerfile", "api/Gemfile", "api/Gemfile.lock", "api/package.json"]
```

## 项目与 Compose

```toml
[coast]
name = "acme"
compose = "./infra/docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "bash"]
run = ["npm install -g yarn"]
```

`compose` 字段指向你现有的 Docker Compose 文件。Coast 在执行 `coast run` 时会在 DinD 容器内运行 `docker compose up -d`，因此你的后端服务（Rails 服务器、Sidekiq worker、SSR 进程）会自动启动。

`[coast.setup]` 在 DinD 宿主机本身安装软件包——而不是在你的 compose 容器内安装。这些包是运行在宿主机上的裸服务（Vite 开发服务器）所必需的。你的 compose 服务仍然像往常一样通过各自的 Dockerfile 获取运行时环境。

## 共享服务

```toml
[shared_services.db]
image = "postgres:15.3-alpine"
ports = [5432]
volumes = ["infra_postgres:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "password" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis:/data"]
```

Postgres 与 Redis 被声明为[共享服务](../concepts_and_terminology/SHARED_SERVICES.md)，而不是在每个 Coast 内部各自运行。这意味着它们运行在宿主机 Docker daemon 上，每个 Coast 实例通过桥接网络连接到它们。

**为什么使用共享服务而不是 compose 内部数据库？**

- **实例更轻量。** 每个 Coast 都跳过启动自己的 Postgres 与 Redis 容器，从而节省内存与启动时间。
- **复用宿主机卷。** `volumes` 字段引用你现有的 Docker 卷（由你本地执行 `docker-compose up` 创建的那些）。你已有的所有数据会立刻可用——无需重新 seed，也无需重新跑迁移。
- **MCP 兼容性。** 如果你在宿主机上有连接到 `localhost:5432` 的数据库 MCP 工具，它们会继续工作，因为共享的 Postgres 就在宿主机同一端口上。无需重新配置。

**代价:** Coast 实例之间没有数据隔离。每个实例都会读写同一个数据库。如果你的工作流需要每个实例独立数据库，请改用 [volume strategies](../concepts_and_terminology/VOLUMES.md) 并设置 `strategy = "isolated"`；或在共享服务上使用 `auto_create_db = true`，在共享 Postgres 内为每个实例创建独立数据库。详情参见 [Shared Services Coastfile reference](../coastfiles/SHARED_SERVICES.md)。

**卷命名很重要。** 卷名（`infra_postgres`、`infra_redis`）必须与你在宿主机上通过本地运行 `docker-compose up` 已存在的卷名称一致。如果不匹配，共享服务将使用一个空卷启动。在编写该部分之前，先运行 `docker volume ls` 检查现有卷名称。

## 裸服务

```toml
[services.vite-web]
install = "cd /workspace && yarn install --immutable 2>/dev/null || yarn install"
command = "cd /workspace && yarn workspace @acme/web run dev"
port = 3040
restart = "on-failure"
cache = ["node_modules"]

[services.vite-api]
command = "cd /workspace && yarn workspace @acme/api run dev"
port = 3036
restart = "on-failure"
```

Vite 开发服务器被定义为[裸服务](../concepts_and_terminology/BARE_SERVICES.md)——在 DinD 宿主机上直接运行的普通进程，不在 Docker Compose 内。这就是[混合服务类型](../concepts_and_terminology/MIXED_SERVICE_TYPES.md)模式。

**为什么用裸服务而不是 compose？**

首要原因是网络。需要访问 Vite 开发服务器的 compose 服务（用于 SSR、资源代理或 HMR WebSocket 连接）可以通过 `host.docker.internal` 访问 DinD 宿主机上的裸服务。这避免了复杂的 Docker 网络配置，并与大多数 monorepo 对 `VITE_RUBY_HOST` 或类似环境变量的配置方式一致。

裸服务还能直接访问 bind-mounted 的 `/workspace` 文件系统，而无需经过内层容器的 overlay。这意味着 Vite 的文件监听对变更的响应更快。

**`install` 与 `cache`:** `install` 字段会在服务启动前运行，并且在每次 `coast assign` 时再次运行。这里它执行 `yarn install`，以便在切换分支时获取依赖变更。`cache` 字段告诉 Coast 在 worktree 切换之间保留 `node_modules`，使得安装是增量的而不是每次从零开始。

**只需要一个 `install`:** 注意 `vite-api` 没有 `install` 字段。在 yarn workspaces 的 monorepo 中，在根目录执行一次 `yarn install` 就会为所有 workspace 安装依赖。只放在一个服务上可避免重复运行两次。

## 端口与健康检查

```toml
[ports]
api = 3000
web = 3002
vite-web = 3040
vite-api = 3036

[healthcheck]
web = "/"
api = "/"
```

你希望 Coast 管理的每个端口都应写在 `[ports]` 中。每个实例都会为每个声明的端口获得一个[动态端口](../concepts_and_terminology/PORTS.md)（高端口范围，始终可访问）。[checked-out](../concepts_and_terminology/CHECKOUT.md) 实例还会获得标准端口（你声明的那个数字）转发到宿主机。

`[healthcheck]` 部分告诉 Coast 如何探测每个端口的健康状态。对于配置了健康检查路径的端口，Coast 每 5 秒发送一次 HTTP GET——任何 HTTP 响应都算健康。没有配置健康检查路径的端口会回退为 TCP 连接检查（端口能否接受连接？）。

在这个例子中，Rails Web 服务器使用 `/` 的 HTTP 健康检查，因为它们提供 HTML 页面。Vite 开发服务器则不配置健康检查路径——它们不会提供有意义的根页面，使用 TCP 检查足以确认它们在接受连接。

健康检查状态可在 [Coastguard](../concepts_and_terminology/COASTGUARD.md) UI 中查看，也可通过 `coast ports` 查看。

## 卷

```toml
[volumes.bundle]
strategy = "shared"
service = "api-rails"
mount = "/usr/local/bundle"

[volumes.api_rails_cache]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/tmp/cache"

[volumes.api_assets]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/public/assets"

[volumes.web_rails_cache]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/tmp/cache"

[volumes.web_assets]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/public/assets"
```

这里所有卷都使用 `strategy = "shared"`，表示一个 Docker 卷会在所有 Coast 实例之间共享。这对**缓存与构建产物**是正确选择——并发写入是安全的，而且为每个实例复制一份会浪费磁盘空间并拖慢启动:

- **`bundle`** — Ruby gem 缓存。各分支的 gem 相同。共享可以避免每个 Coast 实例都重新下载完整 bundle。
- **`*_rails_cache`** — Rails 基于文件的缓存。可加速开发但并不重要——任何实例都能重新生成。
- **`*_assets`** — 编译后的资源。理由与缓存相同。

**为什么数据库不使用 shared？** 如果你在挂载到类数据库服务的卷上使用 `strategy = "shared"`，Coast 会输出警告。多个 Postgres 进程写入同一个数据目录会导致损坏。对于数据库，要么使用[共享服务](../coastfiles/SHARED_SERVICES.md)（像本配方一样，在宿主机上只运行一个 Postgres），要么使用 `strategy = "isolated"`（每个 Coast 拥有独立卷）。完整的决策矩阵请参见 [Volume Topology](../concepts_and_terminology/VOLUMES.md) 页面。

## Assign 策略

```toml
[assign]
default = "none"
exclude_paths = [
    ".coasts",
    ".yarn",
    ".github",
    "docs",
    "scripts",
    "cli",
    "deploy",
    "tools",
    "mobile",
    "extensions",
]

[assign.services]
web-rails = "hot"
web-ssr = "hot"
web-sidekiq = "restart"
api-rails = "hot"
api-sidekiq = "restart"

[assign.rebuild_triggers]
web-rails = ["infra/Dockerfile", "web/Gemfile", "web/Gemfile.lock", "web/package.json"]
api-rails = ["infra/Dockerfile", "api/Gemfile", "api/Gemfile.lock", "api/package.json"]
```

`[assign]` 部分控制你运行 `coast assign` 将某个 Coast 实例切换到不同 worktree 时，各服务会发生什么。把这里配置好，是 5 秒切分支与 60 秒切分支的关键差别。

### `default = "none"`

将默认值设置为 `"none"` 表示:在 `[assign.services]` 中未明确列出的任何服务，在切换分支时都保持不动。这对数据库与缓存至关重要——Postgres、Redis 与基础设施服务在分支之间不会变化，重启它们只是浪费。

### 按服务策略

| Service | Strategy | Why |
|---|---|---|
| `web-rails`, `web-ssr`, `api-rails` | `hot` | 这些运行带文件监听的开发服务器。[filesystem remount](../concepts_and_terminology/FILESYSTEM.md) 会在 `/workspace` 下替换代码，监听器会自动捕获变更。无需重启容器。 |
| `web-sidekiq`, `api-sidekiq` | `restart` | 后台 worker 在启动时加载代码，并不会监听文件变化。它们需要重启容器才能加载新分支的代码。 |

只列出实际在运行的服务。如果你的 `COMPOSE_PROFILES` 只启动部分服务，就不要列出未激活的服务——Coast 会对每个已列出的服务计算 assign 策略，而重启一个未运行的服务是浪费工作。更多内容参见 [Performance Optimizations](../concepts_and_terminology/PERFORMANCE_OPTIMIZATIONS.md)。

### `exclude_paths`

这是大型 monorepo 中影响最大的单项优化。它告诉 Coast 在每次 assign 运行的 gitignored 文件同步（rsync）以及 `git ls-files` diff 期间，跳过整个目录树。

目标是排除你的 Coast 服务不需要的一切。在一个包含 30,000 个文件的 monorepo 中，上述目录可能包含 8,000+ 个与运行中服务无关的文件。排除它们能在每次切分支时减少大量文件 stat。

要找出应排除的内容，先对你的仓库做 profiling:

```bash
git ls-files | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

保留包含会挂载到运行服务中的源代码的目录，或被这些服务导入的共享库目录。排除其余所有内容——文档、CI 配置、工具链、其他团队的应用、移动端客户端、CLI 工具，以及像 `.yarn` 这样的 vendored 缓存。

### `rebuild_triggers`

没有触发器时，`strategy = "rebuild"` 的服务会在每次切分支时都重建其 Docker 镜像——即使没有任何会影响镜像的内容发生变化。`[assign.rebuild_triggers]` 部分将重建限制在特定文件发生变化时才进行。

在本配方中，Rails 服务通常使用 `"hot"`（完全不重启）。但如果有人修改了 Dockerfile 或 Gemfile，`rebuild_triggers` 会介入并强制完整镜像重建。如果触发文件都没有变化，Coast 会完全跳过重建。这避免了日常代码变更带来的昂贵镜像构建，同时仍能捕捉到基础设施层面的变更。

## Secrets 与 Inject

```toml
[secrets.compose_profiles]
extractor = "command"
run = "echo api,web"
inject = "env:COMPOSE_PROFILES"

[secrets.uid]
extractor = "command"
run = "id -u"
inject = "env:UID"

[secrets.gid]
extractor = "command"
run = "id -g"
inject = "env:GID"

[inject]
env = ["USER", "BUNDLE_GEMS__CONTRIBSYS__COM"]
```

`[secrets]` 部分在构建时提取值，并将其作为环境变量注入到 Coast 实例中。

- **`compose_profiles`** 控制启动哪些 Docker Compose profile。这用于限制某个 Coast 只运行 `api` 与 `web` profile，而不是启动 compose 文件里定义的全部服务。你可以在宿主机上在构建前通过 `export COMPOSE_PROFILES=api,web,portal` 覆盖它，以改变启动哪些服务。
- **`uid` / `gid`** 将宿主机用户的 UID 与 GID 传入容器，这在需要宿主机与容器之间文件所有权一致的 Docker 方案中很常见。

`[inject]` 部分更简单——它在运行时将宿主机已有的环境变量转发进 Coast 容器。敏感凭据（例如 gem 服务器 token:`BUNDLE_GEMS__CONTRIBSYS__COM`）保留在宿主机上，并被转发而不会写入任何配置文件。

关于 secret extractor 与注入目标的完整参考，请参见 [Secrets](../coastfiles/SECRETS.md)。

## 调整此配方以适配你的项目

**不同语言栈:** 将 Rails 相关的卷（bundle、rails cache、assets）替换为你技术栈的对应项——Go 模块缓存（`/go/pkg/mod`）、npm 缓存、pip 缓存等。对于任何可在实例间安全共享的缓存，策略仍保持 `"shared"`。

**更少的应用:** 如果你的 monorepo 只有一个应用，删掉多余的卷条目，并简化 `[assign.services]`，只列出你的服务即可。共享服务与裸服务模式仍然适用。

**每实例数据库:** 如果你需要 Coast 实例之间的数据隔离，用 compose 内部的 Postgres 替换 `[shared_services.db]`，并添加一个设置了 `strategy = "isolated"` 的 `[volumes]` 条目。这样每个实例会获得自己的数据库卷。你可以用 `snapshot_source` 从宿主机卷进行初始化——参见 [Volumes Coastfile reference](../coastfiles/VOLUMES.md)。

**不使用裸服务:** 如果你的前端已完全容器化，并且不需要通过 `host.docker.internal` 被访问，移除 `[services.*]` 部分以及 `[coast.setup]`。所有内容都通过 compose 运行。
