# Coastfile 类型

一个项目可以为不同的使用场景拥有多个 Coastfile。每个变体都称为一种“type”。类型让你可以组合共享通用基础、但在运行哪些服务、如何处理卷，或服务是否自动启动等方面有所不同的配置。

## 类型如何工作

命名约定是:默认使用 `Coastfile`，变体使用 `Coastfile.{type}`。点号后的后缀会成为类型名:

- `Coastfile` -- 默认类型
- `Coastfile.test` -- test 类型
- `Coastfile.snap` -- snapshot 类型
- `Coastfile.light` -- lightweight 类型

任何 Coastfile 都可以选择使用 `.toml` 扩展名以获得编辑器语法高亮。在推导类型之前会先去掉 `.toml` 后缀，因此以下这些是等价的文件对:

- `Coastfile.toml` = `Coastfile`（默认类型）
- `Coastfile.test.toml` = `Coastfile.test`（test 类型）
- `Coastfile.light.toml` = `Coastfile.light`（lightweight 类型）

**平局规则:**如果两种形式同时存在（例如 `Coastfile` 和 `Coastfile.toml`，或 `Coastfile.light` 和 `Coastfile.light.toml`），则优先使用 `.toml` 变体。

**保留类型名:**`"default"` 和 `"toml"` 不能用作类型名。`Coastfile.default` 和 `Coastfile.toml`（作为类型后缀时，表示一个字面名称为 `Coastfile.toml.toml` 的文件）都会被拒绝。

你可以使用 `--type` 构建并运行带类型的 Coast:

```bash
coast build --type test
coast run test-1 --type test
coast exec test-1 -- go test ./...
```

## extends

带类型的 Coastfile 通过 `extends` 继承父级。父级中的所有内容都会被合并进来。子级只需要指定它要覆盖或新增的内容。

```toml
[coast]
extends = "Coastfile"
```

这样可以避免为每个变体重复整个配置。子级会继承父级中的所有 [ports](PORTS.md)、[secrets](SECRETS.md)、[volumes](VOLUMES.md)、[shared services](SHARED_SERVICES.md)、[assign strategies](ASSIGN.md)、setup 命令以及 [MCP](MCP_SERVERS.md) 配置。凡是子级定义的内容，都会优先于父级。

## [unset]

按名称移除从父级继承的特定项。你可以 unset `ports`、`shared_services`、`secrets` 和 `volumes`。

```toml
[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]
```

这就是测试变体移除共享服务（这样数据库就在 Coast 内部运行并使用隔离卷）并删除其不需要端口的方式。

## [omit]

将 compose 服务从构建中完全剥离。被省略的服务会从 compose 文件中移除，并且根本不会在 Coast 内运行。

```toml
[omit]
services = ["redis", "backend", "mailhog", "web"]
```

用它来排除与该变体用途无关的服务。一个测试变体可能只保留数据库、迁移和测试运行器。

## autostart

控制 Coast 启动时是否自动运行 `docker compose up`。默认值是 `true`。

```toml
[coast]
extends = "Coastfile"
autostart = false
```

对于那些你希望手动运行特定命令、而不是拉起整个栈的变体，请设置 `autostart = false`。这在测试运行器中很常见 —— 你先创建 Coast，然后使用 [`coast exec`](EXEC_AND_DOCKER.md) 运行单独的测试套件。

## 常见模式

### 测试变体

一个 `Coastfile.test`，只保留运行测试所需的内容:

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

每个测试 Coast 都会得到自己的干净数据库。不会暴露任何端口，因为测试通过内部 compose 网络与服务通信。`autostart = false` 表示你使用 `coast exec` 手动触发测试运行。

### 快照变体

一个 `Coastfile.snap`，用主机现有数据库卷的副本为每个 Coast 进行初始化:

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

共享服务被 unset，因此数据库会在每个 Coast 内部运行。`snapshot_source` 会在构建时从现有主机卷为隔离卷提供初始数据。创建之后，每个实例的数据都会独立分化。

### 轻量级变体

一个 `Coastfile.light`，将项目精简到特定工作流所需的最小集合 —— 例如，仅保留一个后端服务及其数据库，以便快速迭代。

## 独立构建池

每种类型都有自己的 `latest-{type}` 符号链接，以及自己的 5 个构建自动清理池:

```bash
coast build              # 更新 latest，清理 default 构建
coast build --type test  # 更新 latest-test，清理 test 构建
coast build --type snap  # 更新 latest-snap，清理 snap 构建
```

构建 `test` 类型不会影响 `default` 或 `snap` 构建。清理是按类型完全独立进行的。

## 运行带类型的 Coast

使用 `--type` 创建的实例会被标记其类型。你可以为同一个项目同时运行不同类型的实例:

```bash
coast run dev-1                    # 默认类型
coast run test-1 --type test       # test 类型
coast run snapshot-1 --type snap   # snapshot 类型

coast ls
# 三者都会显示出来，每个都有自己的类型、端口和卷策略
```

这就是为什么你可以在同一个项目中，同时运行完整的开发环境、隔离的测试运行器以及基于快照初始化的实例，而且它们都可以在同一时间并行运行。
