# Coastfile 类型

一个项目可以针对不同的使用场景拥有多个 Coastfile。每个变体称为一种“类型（type）”。类型让你可以组合共享同一基础配置但在运行哪些服务、如何处理卷、或服务是否自动启动方面有所不同的配置。

## 类型如何工作

命名约定为:默认使用 `Coastfile`，变体使用 `Coastfile.{type}`。点号后面的后缀就是类型名称:

- `Coastfile` -- 默认类型
- `Coastfile.test` -- 测试类型
- `Coastfile.snap` -- 快照类型
- `Coastfile.light` -- 轻量类型

你可以使用 `--type` 来构建并运行带类型的 Coast:

```bash
coast build --type test
coast run test-1 --type test
coast exec test-1 -- go test ./...
```

## extends

带类型的 Coastfile 通过 `extends` 从父级继承。父级中的所有内容都会被合并进来。子级只需要指定它要覆盖或新增的内容。

```toml
[coast]
extends = "Coastfile"
```

这避免了为每个变体重复整套配置。子级会从父级继承所有 [ports](PORTS.md)、[secrets](SECRETS.md)、[volumes](VOLUMES.md)、[shared services](SHARED_SERVICES.md)、[assign strategies](ASSIGN.md)、setup 命令以及 [MCP](MCP_SERVERS.md) 配置。子级定义的任何内容都会优先于父级。

## [unset]

按名称移除从父级继承的特定条目。你可以 unset `ports`、`shared_services`、`secrets` 和 `volumes`。

```toml
[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]
```

这就是测试变体如何移除 shared services（让数据库在 Coast 内部运行并使用隔离卷），以及移除它不需要的端口。

## [omit]

从构建中彻底剔除 compose 服务。被 omit 的服务会从 compose 文件中移除，并且根本不会在 Coast 内运行。

```toml
[omit]
services = ["redis", "backend", "mailhog", "web"]
```

用它来排除与该变体目的无关的服务。测试变体可能只保留数据库、迁移以及测试运行器。

## autostart

控制 Coast 启动时是否自动运行 `docker compose up`。默认值为 `true`。

```toml
[coast]
extends = "Coastfile"
autostart = false
```

对于你希望手动运行特定命令、而不是拉起完整堆栈的变体，请设置 `autostart = false`。这在测试运行器中很常见——你先创建 Coast，然后使用 [`coast exec`](EXEC_AND_DOCKER.md) 运行单独的测试套件。

## 常见模式

### 测试变体

一个 `Coastfile.test`，只保留运行测试所需内容:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]

[omit]
services = ["redis", "backend", "mailhog", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[assign]
default = "none"
[assign.services]
test-runner = "rebuild"
migrations = "rebuild"
```

每个测试 Coast 都会拥有自己干净的数据库。不会暴露任何端口，因为测试通过内部 compose 网络与服务通信。`autostart = false` 表示你使用 `coast exec` 手动触发测试运行。

### 快照变体

一个 `Coastfile.snap`，在创建每个 Coast 时，用主机上现有数据库卷的副本进行初始化:

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis"]

[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "my_project_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "my_project_redis_data"
service = "redis"
mount = "/data"
```

通过 unset shared services，让数据库在每个 Coast 内部运行。`snapshot_source` 会在构建时从现有主机卷为隔离卷提供初始数据。创建后，每个实例的数据都会各自独立地分化。

### 轻量变体

一个 `Coastfile.light`，将项目裁剪到特定工作流的最小集合——例如只保留一个后端服务及其数据库，以便快速迭代。

## 独立的构建池

每种类型都有自己的 `latest-{type}` 符号链接，以及各自 5 个构建的自动清理池:

```bash
coast build              # 更新 latest，清理默认构建
coast build --type test  # 更新 latest-test，清理 test 构建
coast build --type snap  # 更新 latest-snap，清理 snap 构建
```

构建 `test` 类型不会影响 `default` 或 `snap` 的构建。清理对每种类型而言完全独立。

## 运行带类型的 Coasts

使用 `--type` 创建的实例会被标记为对应类型。你可以让同一项目的不同类型实例同时运行:

```bash
coast run dev-1                    # 默认类型
coast run test-1 --type test       # 测试类型
coast run snapshot-1 --type snap   # 快照类型

coast ls
# 三者都会出现，每个都有自己的类型、端口和卷策略
```

这样你就可以让完整的开发环境与隔离的测试运行器和基于快照初始化的实例并行运行——同一个项目、同一时间、同时存在。
