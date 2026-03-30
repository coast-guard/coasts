# 继承、类型和组合

Coastfile 支持继承（`extends`）、片段组合（`includes`）、条目移除（`[unset]`）以及 compose 级别剥离（`[omit]`）。这些功能结合在一起，使你能够只定义一次基础配置，并为不同工作流创建精简变体——测试运行器、轻量级前端、快照预置栈——而无需重复配置。

有关带类型 Coastfile 如何融入构建系统的更高层概述，请参阅 [Coastfile Types](../concepts_and_terminology/COASTFILE_TYPES.md) 和 [Builds](../concepts_and_terminology/BUILDS.md)。

## Coastfile 类型

基础 Coastfile 始终命名为 `Coastfile`。类型变体使用命名模式 `Coastfile.{type}`:

- `Coastfile` —— 默认类型
- `Coastfile.light` —— 类型 `light`
- `Coastfile.snap` —— 类型 `snap`
- `Coastfile.ci.minimal` —— 类型 `ci.minimal`

任何 Coastfile 都可以带有可选的 `.toml` 扩展名，以便编辑器进行语法高亮。提取类型之前会去掉 `.toml` 后缀:

- `Coastfile.toml` = `Coastfile`（默认类型）
- `Coastfile.light.toml` = `Coastfile.light`（类型 `light`）
- `Coastfile.ci.minimal.toml` = `Coastfile.ci.minimal`（类型 `ci.minimal`）

如果普通形式和 `.toml` 形式同时存在（例如 `Coastfile` 和 `Coastfile.toml`），则 `.toml` 变体优先。

名称 `Coastfile.default` 和 `"toml"`（作为类型）是保留的，不允许使用。尾随点号（`Coastfile.`）同样无效。

使用 `--type` 构建和运行类型变体:

```
coast build --type light
coast run test-1 --type light
```

每种类型都有其各自独立的构建池。`--type light` 构建不会干扰默认构建。

## `extends`

类型 Coastfile 可以在 `[coast]` 部分使用 `extends` 继承父级。会先完整解析父级，然后再将子级的值叠加到其上。

```toml
[coast]
extends = "Coastfile"
```

该值是指向父 Coastfile 的相对路径，相对于子文件所在目录解析。如果精确路径不存在，Coast 还会尝试追加 `.toml`——因此如果磁盘上只存在 `.toml` 变体，`extends = "Coastfile"` 也会找到 `Coastfile.toml`。支持链式继承——子级可以继承一个本身又继承祖先的父级:

```
Coastfile                    (base)
  └─ Coastfile.light         (extends Coastfile)
       └─ Coastfile.chain    (extends Coastfile.light)
```

循环链（A 继承 B，B 又继承 A，或者 A 继承 A）会被检测并拒绝。

### 合并语义

当子级继承父级时:

- **标量字段**（`name`、`runtime`、`compose`、`root`、`worktree_dir`、`autostart`、`primary_port`）——如果子级存在该值，则子级胜出；否则继承父级。
- **映射**（`[ports]`、`[egress]`）——按键合并。子级键会覆盖同名父级键；仅存在于父级的键会被保留。
- **命名部分**（`[secrets.*]`、`[volumes.*]`、`[shared_services.*]`、`[mcp.*]`、`[mcp_clients.*]`、`[services.*]`）——按名称合并。子级中同名条目会完全替换父级条目；新名称会被添加。
- **`[coast.setup]`**:
  - `packages` —— 去重后的并集（子级添加新包，父级包会被保留）
  - `run` —— 子级命令会追加在父级命令之后
  - `files` —— 按 `path` 合并（相同路径 = 子级条目替换父级条目）
- **`[inject]`** —— `env` 和 `files` 列表会拼接。
- **`[omit]`** —— `services` 和 `volumes` 列表会拼接。
- **`[assign]`** —— 如果子级中存在，则整体替换（而不是逐字段合并）。
- **`[agent_shell]`** —— 如果子级中存在，则整体替换。

### 继承项目名称

如果子级未设置 `name`，则会继承父级的名称。这对于类型变体来说是正常的——它们是同一项目的变体:

```toml
# Coastfile
[coast]
name = "my-app"
```

```toml
# Coastfile.light — inherits name "my-app"
[coast]
extends = "Coastfile"
autostart = false
```

如果你希望该变体显示为一个单独的项目，可以在子级中覆盖 `name`:

```toml
[coast]
extends = "Coastfile"
name = "my-app-light"
```

## `includes`

`includes` 字段会在应用文件自身的值之前，将一个或多个 TOML 片段文件合并到 Coastfile 中。这对于将共享配置（如一组 secrets 或 MCP 服务器）提取到可复用片段中非常有用。

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]
```

被包含的片段是一个 TOML 文件，具有与 Coastfile 相同的部分结构。它必须包含一个 `[coast]` 部分（可以为空），但其自身不能使用 `extends` 或 `includes`。

```toml
# extra-secrets.toml
[coast]

[secrets.mongo_uri]
extractor = "env"
var = "MONGO_URI"
inject = "env:MONGO_URI"
```

当同时存在 `extends` 和 `includes` 时，合并顺序如下:

1. 递归解析父级（通过 `extends`）
2. 按顺序合并每个被包含的片段
3. 应用文件自身的值（其优先级高于其他所有内容）

## `[unset]`

在所有合并完成后，从已解析配置中移除命名条目。这就是子级在不必重新定义整个部分的情况下，移除从父级继承内容的方式。

```toml
[unset]
secrets = ["db_password"]
shared_services = ["postgres", "redis"]
ports = ["postgres", "redis"]
```

支持的字段:

- `secrets` —— 要移除的 secret 名称列表
- `ports` —— 要移除的端口名称列表
- `shared_services` —— 要移除的共享服务名称列表
- `volumes` —— 要移除的卷名称列表
- `mcp` —— 要移除的 MCP 服务器名称列表
- `mcp_clients` —— 要移除的 MCP 客户端名称列表
- `egress` —— 要移除的 egress 名称列表
- `services` —— 要移除的裸服务名称列表

`[unset]` 会在完整的 extends + includes 合并链解析完成后应用。它会按名称从最终合并结果中移除条目。

## `[omit]`

从在 Coast 内部运行的 Docker Compose 栈中剥离 compose 服务和卷。不同于 `[unset]`（它移除的是 Coastfile 级别配置），`[omit]` 告诉 Coast 在 DinD 容器内运行 `docker compose up` 时排除特定服务或卷。

```toml
[omit]
services = ["monitoring", "debug-tools", "nginx-proxy"]
volumes = ["keycloak-db-data"]
```

- **`services`** —— 要从 `docker compose up` 中排除的 compose 服务名称
- **`volumes`** —— 要排除的 compose 卷名称

当你的 `docker-compose.yml` 定义了并非每个 Coast 变体都需要的服务时，这会很有用——例如监控栈、反向代理、管理工具。你无需维护多个 compose 文件，而是使用单个 compose 文件，并按变体剥离不需要的内容。

当子级继承父级时，`[omit]` 列表会拼接——子级会向父级的 omit 列表中添加内容。

## 示例

### 轻量级测试变体

继承基础 Coastfile，禁用自动启动，剥离共享服务，并按实例隔离运行数据库:

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

### 快照预置变体

从基础配置中移除共享服务，并用基于快照预置的隔离卷替换它们:

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

### 带有额外共享服务和 includes 的类型变体

继承基础配置，添加 MongoDB，并从片段中拉取额外 secrets:

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

三层深度:base -> light -> chain。

```toml
# Coastfile.chain
[coast]
extends = "Coastfile.light"

[coast.setup]
run = ["echo 'chain setup appended'"]

[ports]
debug = 39999
```

已解析配置会从基础 `Coastfile` 开始，在其上合并 `Coastfile.light`，然后再在其上合并 `Coastfile.chain`。来自三个层级的 setup `run` 命令会按顺序拼接。setup `packages` 会在所有层级中去重。

### 从大型 compose 栈中省略服务

从 `docker-compose.yml` 中剥离开发时不需要的服务:

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
