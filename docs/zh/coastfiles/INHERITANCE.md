# 继承、类型与组合

Coastfile 支持继承（`extends`）、片段组合（`includes`）、条目移除（`[unset]`）以及 compose 级别裁剪（`[omit]`）。这些功能结合起来，让你只需定义一次基础配置，就能为不同工作流创建精简变体——测试运行器、轻量前端、快照播种的栈——而无需重复配置。

有关类型化 Coastfile 如何融入构建系统的更高层概览，请参阅 [Coastfile Types](../concepts_and_terminology/COASTFILE_TYPES.md) 和 [Builds](../concepts_and_terminology/BUILDS.md)。

## Coastfile 类型

基础 Coastfile 始终命名为 `Coastfile`。类型化变体使用命名模式 `Coastfile.{type}`:

- `Coastfile` — 默认类型
- `Coastfile.light` — 类型 `light`
- `Coastfile.snap` — 类型 `snap`
- `Coastfile.ci.minimal` — 类型 `ci.minimal`

名称 `Coastfile.default` 被保留且不允许使用。尾随点（`Coastfile.`）也无效。

使用 `--type` 构建并运行类型化变体:

```
coast build --type light
coast run test-1 --type light
```

每种类型都有其各自独立的构建池。`--type light` 的构建不会干扰默认构建。

## `extends`

类型化 Coastfile 可以在 `[coast]` 段中使用 `extends` 从父级继承。父级会先被完整解析，然后子级的值再叠加到其上。

```toml
[coast]
extends = "Coastfile"
```

该值是父 Coastfile 的相对路径，相对于子文件所在目录解析。支持链式继承——子级可以扩展一个父级，而该父级本身又扩展了祖父级:

```
Coastfile                    (base)
  └─ Coastfile.light         (extends Coastfile)
       └─ Coastfile.chain    (extends Coastfile.light)
```

循环链（A extends B extends A，或 A extends A）会被检测并拒绝。

### 合并语义

当子级扩展父级时:

- **标量字段**（`name`, `runtime`, `compose`, `root`, `worktree_dir`, `autostart`, `primary_port`）——若子级提供则子级优先；否则从父级继承。
- **映射**（`[ports]`, `[egress]`）——按键合并。子级键会覆盖同名父级键；仅存在于父级的键会保留。
- **命名段**（`[secrets.*]`, `[volumes.*]`, `[shared_services.*]`, `[mcp.*]`, `[mcp_clients.*]`, `[services.*]`）——按名称合并。子级中同名条目会完全替换父级条目；新名称会被添加。
- **`[coast.setup]`**:
  - `packages` —— 去重后的并集（子级添加新包，父级包保留）
  - `run` —— 子级命令会追加在父级命令之后
  - `files` —— 按 `path` 合并（相同 path = 子级条目替换父级条目）
- **`[inject]`** —— `env` 与 `files` 列表会被拼接。
- **`[omit]`** —— `services` 与 `volumes` 列表会被拼接。
- **`[assign]`** —— 若子级中存在则整体替换（不逐字段合并）。
- **`[agent_shell]`** —— 若子级中存在则整体替换。

### 继承项目名称

如果子级未设置 `name`，它会继承父级的名称。这对于类型化变体来说很正常——它们是同一项目的变体:

```toml
# Coastfile
[coast]
name = "my-app"
```

```toml
# Coastfile.light — 继承 name "my-app"
[coast]
extends = "Coastfile"
autostart = false
```

如果你希望该变体显示为一个独立项目，可以在子级中覆盖 `name`:

```toml
[coast]
extends = "Coastfile"
name = "my-app-light"
```

## `includes`

`includes` 字段会在应用该文件自身的值之前，将一个或多个 TOML 片段文件合并进 Coastfile。这对于将共享配置（例如一组 secrets 或 MCP 服务器）提取到可复用片段中很有用。

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]
```

被包含的片段是一个 TOML 文件，具有与 Coastfile 相同的段结构。它必须包含一个 `[coast]` 段（可以为空），但其自身不能使用 `extends` 或 `includes`。

```toml
# extra-secrets.toml
[coast]

[secrets.mongo_uri]
extractor = "env"
var = "MONGO_URI"
inject = "env:MONGO_URI"
```

当同时存在 `extends` 与 `includes` 时的合并顺序:

1.（通过 `extends`）递归解析父级
2. 按顺序合并每个被包含的片段
3. 应用该文件自身的值（其优先级最高，覆盖一切）

## `[unset]`

在所有合并完成后，从已解析的配置中移除命名条目。这是子级移除从父级继承的内容的方式，而无需重定义整个段。

```toml
[unset]
secrets = ["db_password"]
shared_services = ["postgres", "redis"]
ports = ["postgres", "redis"]
```

支持的字段:

- `secrets` — 要移除的 secret 名称列表
- `ports` — 要移除的端口名称列表
- `shared_services` — 要移除的共享服务名称列表
- `volumes` — 要移除的卷名称列表
- `mcp` — 要移除的 MCP 服务器名称列表
- `mcp_clients` — 要移除的 MCP 客户端名称列表
- `egress` — 要移除的 egress 名称列表
- `services` — 要移除的裸服务名称列表

`[unset]` 会在完整的 extends + includes 合并链解析完成后应用。它通过名称从最终合并结果中移除条目。

## `[omit]`

从运行在 Coast 内部的 Docker Compose 栈中剔除 compose services 与 volumes。不同于 `[unset]`（移除 Coastfile 层级配置），`[omit]` 告诉 Coast 在 DinD 容器内运行 `docker compose up` 时排除特定服务或卷。

```toml
[omit]
services = ["monitoring", "debug-tools", "nginx-proxy"]
volumes = ["keycloak-db-data"]
```

- **`services`** — 要从 `docker compose up` 中排除的 compose 服务名称
- **`volumes`** — 要排除的 compose 卷名称

当你的 `docker-compose.yml` 定义了并非每个 Coast 变体都需要的服务时，这很有用——监控栈、反向代理、管理工具。与其维护多个 compose 文件，不如使用单一 compose 文件，并按变体剔除不需要的部分。

当子级扩展父级时，`[omit]` 列表会被拼接——子级在父级的 omit 列表基础上继续添加。

## 示例

### 轻量测试变体

扩展基础 Coastfile，禁用自动启动，剔除共享服务，并使数据库按实例隔离运行:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "backend", "postgres", "redis"]
shared_services = ["postgres", "redis", "mongodb"]

[omit]
services = ["redis", "backend", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[assign]
default = "none"
[assign.services]
backend-test = "rebuild"
migrations = "rebuild"
```

### 快照播种变体

从基础配置中移除共享服务，并用由快照播种的隔离卷替换它们:

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

### 带额外共享服务与 includes 的类型化变体

扩展基础配置，添加 MongoDB，并从片段中引入额外 secrets:

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]

[ports]
mongodb = 37017

[shared_services.mongodb]
image = "mongo:7"
ports = [27017]
env = { MONGO_INITDB_ROOT_USERNAME = "dev", MONGO_INITDB_ROOT_PASSWORD = "dev" }

[omit]
services = ["debug-tools"]
```

### 多级继承链

三层:base -> light -> chain。

```toml
# Coastfile.chain
[coast]
extends = "Coastfile.light"

[coast.setup]
run = ["echo 'chain setup appended'"]

[ports]
debug = 39999
```

解析后的配置以基础 `Coastfile` 开始，在其上合并 `Coastfile.light`，再在其上合并 `Coastfile.chain`。来自三层的 setup `run` 命令会按顺序拼接。Setup `packages` 会在所有层级之间去重。

### 从大型 compose 栈中剔除服务

从 `docker-compose.yml` 中剔除开发不需要的服务:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[omit]
services = ["backend-debug", "backend-debug-test", "asynqmon", "postgres-keycloak", "keycloak", "redash-db-init", "redash-init", "redash", "redash-scheduler", "redash-worker", "langfuse-db-init", "langfuse", "nginx-proxy"]
volumes = ["keycloak-db-data"]

[ports]
web = 3000
backend = 8080
```
