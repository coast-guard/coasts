# 共享服务

`[shared_services.*]` 部分定义 Coast 项目会使用的基础设施服务——数据库、缓存、消息代理。它有两种形式:

- **内联** —— 直接在使用方 Coastfile 中声明 `image`、`ports`、`env`、`volumes`。Coast 会在宿主机侧启动一个容器，并将使用方应用的流量路由到该容器。最适合只有一个使用方实例的单人项目，或非常轻量的服务。
- **来自共享服务组（`from_group = true`）** —— 服务存在于项目的[共享服务组](../shared_service_groups/README.md)中（这是一个在 `Coastfile.shared_service_groups` 中声明的独立 DinD 容器）。使用方 Coastfile 只需选择启用它。最适合你需要 secret 提取、宿主机侧 checkout 到规范端口，或者你在这台宿主机上运行多个 Coast 项目且每个项目都需要同一个规范端口时使用（SSG 会让 Postgres 保持在内部 `:5432`，而不绑定宿主机 5432，因此两个项目可以共存）。

本页的两个部分将依次说明这两种形式。

关于共享服务在运行时如何工作、生命周期管理以及故障排查，请参阅[共享服务（概念）](../concepts_and_terminology/SHARED_SERVICES.md)。

---

## 内联共享服务

每个内联服务都是 `[shared_services]` 下一个具名的 TOML section。`image` 字段是必需的；其他所有字段都是可选的。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image`（必需）

要在宿主机守护进程上运行的 Docker 镜像。

### `ports`

服务暴露的端口列表。Coast 同时接受裸容器端口，以及 Docker Compose 风格的 `"HOST:CONTAINER"` 映射。

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- 像 `6379` 这样的裸整数是 `"6379:6379"` 的简写。
- 像 `"5433:5432"` 这样的映射字符串会将共享服务发布到宿主机端口 `5433`，同时使其在 Coast 内部仍可通过 `service-name:5432` 访问。
- 宿主机端口和容器端口都必须为非零。

### `volumes`

用于持久化数据的 Docker volume 绑定字符串。这些是宿主机级别的 Docker volumes，而不是由 Coast 管理的 volumes。

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
```

### `env`

传递给服务容器的环境变量。

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_DB = "mydb" }
```

### `auto_create_db`

当为 `true` 时，Coast 会在共享服务内为每个 Coast 实例自动创建一个按实例划分的数据库。默认为 `false`。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
auto_create_db = true
```

### `inject`

将共享服务的连接信息以环境变量或文件的形式注入到 Coast 实例中。使用与 [secrets](SECRETS.md) 相同的 `env:NAME` 或 `file:/path` 格式。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
inject = "env:DATABASE_URL"
```

### 生命周期

当第一个引用某个内联共享服务的 Coast 实例运行时，该共享服务会自动启动。它们会在 `coast stop` 和 `coast rm` 之后继续运行——删除实例不会影响共享服务的数据。只有 `coast shared rm` 才会停止并移除该服务。

由 `auto_create_db` 创建的按实例数据库也会在实例删除后保留。使用 `coast shared-services rm` 来移除该服务并彻底删除其数据。

### 内联示例

#### Postgres、Redis 和 MongoDB

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_MULTIPLE_DATABASES = "dev_db,test_db" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis_data:/data"]

[shared_services.mongodb]
image = "mongo:latest"
ports = [27017]
volumes = ["infra_mongodb_data:/data/db"]
env = { MONGO_INITDB_ROOT_USERNAME = "myapp", MONGO_INITDB_ROOT_PASSWORD = "myapp_pass" }
```

#### 最小共享 Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### 宿主机/容器映射的 Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = ["5433:5432"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### 自动创建的数据库

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

---

## 来自共享服务组的共享服务

对于希望使用结构化共享基础设施设置的项目——多个 worktree、宿主机侧 checkout、SSG 原生 secrets、跨 SSG 重建仍保持虚拟端口——可以在 [`Coastfile.shared_service_groups`](SHARED_SERVICE_GROUPS.md) 中声明一次服务，并在使用方 Coastfile 中通过 `from_group = true` 引用它们:

```toml
[shared_services.postgres]
from_group = true

# Optional per-consumer overrides:
inject = "env:DATABASE_URL"
# auto_create_db = false    # overrides the SSG service's default
```

TOML 键（此示例中的 `postgres`）必须与项目 `Coastfile.shared_service_groups` 中声明的某个服务名称一致。这里引用的 SSG **始终是使用方项目自己的 SSG**（命名为 `<project>-ssg`，其中 `<project>` 是使用方 `[coast].name` 的值）。

### `from_group = true` 时禁止使用的字段

由于 SSG 是唯一的事实来源，以下字段会在解析时被拒绝:

- `image`
- `ports`
- `env`
- `volumes`

当这些字段中的任意一个与 `from_group = true` 同时出现时，会产生:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports, env, volumes.
```

### 允许的按使用方覆盖

- `inject` —— 暴露连接字符串的环境变量或文件路径。不同的使用方 Coastfile 可以将同一个 SSG Postgres 暴露为不同的环境变量名。
- `auto_create_db` —— Coast 是否应在 `coast run` 时为此服务在内部创建一个按实例划分的数据库。它会覆盖 SSG 服务自身的 `auto_create_db` 值。

### 缺失服务错误

如果你引用了一个未在项目 `Coastfile.shared_service_groups` 中声明的名称，`coast build` 会失败:

```text
error: shared service 'postgres' has from_group = true but no service named 'postgres' is declared in Coastfile.shared_service_groups for project 'my-app'.
```

### 何时选择 `from_group` 而不是内联

| 需求 | 内联 | `from_group` |
|---|---|---|
| 此宿主机上只有一个 Coast 项目，且不需要 secrets | 两者都可以；内联更简单 | 可以 |
| **同一**项目的多个 worktree / 使用方实例共享一个 Postgres | 可以（同级实例共享一个宿主机容器） | 可以 |
| 此宿主机上的**两个不同 Coast 项目**都声明了相同的规范端口（例如都想让 Postgres 运行在 5432） | 会在宿主机端口上冲突；无法同时运行 | 必需（每个项目的 SSG 都拥有自己的内部 Postgres，而不绑定宿主机 5432） |
| 希望通过 `coast ssg checkout` 在宿主机侧使用 `psql localhost:5432` | -- | 必需 |
| 需要为该服务在构建时提取 secret（例如从钥匙串中获取 `POSTGRES_PASSWORD`） | -- | 必需（见 [SSG Secrets](../shared_service_groups/SECRETS.md)） |
| 需要跨重建保持稳定的使用方路由（虚拟端口） | -- | 必需（见 [SSG Routing](../shared_service_groups/ROUTING.md)） |

关于完整的 SSG 架构，请参阅[共享服务组](../shared_service_groups/README.md)。关于使用方侧体验，包括自动启动、漂移检测和远程使用方，请参阅[Consuming](../shared_service_groups/CONSUMING.md)。

---

## 另请参阅

- [共享服务（概念）](../concepts_and_terminology/SHARED_SERVICES.md) —— 两种形式的运行时架构
- [共享服务组](../shared_service_groups/README.md) —— SSG 概念总览
- [Coastfile: Shared Service Groups](SHARED_SERVICE_GROUPS.md) —— SSG 侧的 Coastfile schema
- [使用 SSG](../shared_service_groups/CONSUMING.md) —— `from_group = true` 语义的详细说明
