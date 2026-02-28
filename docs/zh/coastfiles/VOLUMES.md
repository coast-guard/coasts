# 卷（Volumes）

`[volumes.*]` 部分控制命名的 Docker 卷在各个 Coast 实例之间如何处理。每个卷都使用一种策略进行配置，该策略决定实例之间是共享数据，还是各自拥有独立的副本。

关于 Coast 中数据隔离的更大图景——包括作为替代方案的共享服务——请参见 [Volumes](../concepts_and_terminology/VOLUMES.md)。

## 定义一个卷

每个卷都是 `[volumes]` 下的一个具名 TOML section。需要三个字段:

- **`strategy`** — `"isolated"` 或 `"shared"`
- **`service`** — 使用该卷的 compose 服务名称
- **`mount`** — 该卷在容器中的挂载路径

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"
```

## 策略

### `isolated`

每个 Coast 实例都会获得自己独立的卷。数据不会在实例之间共享。卷会在 `coast run` 时创建，并在 `coast rm` 时删除。

```toml
[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"
```

这通常是大多数数据库卷的正确选择——每个实例都有一块干净的起点，并且可以自由修改数据而不影响其他实例。

### `shared`

所有 Coast 实例使用同一个 Docker 卷。任一实例写入的数据对所有其他实例都可见。

```toml
[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

共享卷永远不会被 `coast rm` 删除。它们会一直保留，直到你手动移除。

如果你在一个挂载到类似数据库服务的卷上使用 `shared`，Coast 会在构建时打印警告。在多个并发实例之间共享单个数据库卷可能导致损坏。如果你需要共享数据库，请改用 [shared services](SHARED_SERVICES.md)。

共享卷的良好用途:依赖缓存（Go modules、npm cache、pip cache）、构建产物缓存，以及其他并发写入安全或不太可能发生的 data。

## 快照播种（Snapshot seeding）

在实例创建时，隔离卷可以使用 `snapshot_source` 从现有 Docker 卷进行播种。源卷的数据会被复制到新的隔离卷中，随后它们会各自独立地分叉演进。

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "db"
mount = "/var/lib/postgresql/data"
```

`snapshot_source` 只在 `strategy = "isolated"` 时有效。把它设置在共享卷上会报错。

当你希望每个 Coast 实例都以从主机开发数据库复制而来的真实数据集启动，同时又希望实例能自由修改这些数据且不影响源数据或彼此时，这会很有用。

## 示例

### 隔离数据库，共享依赖缓存

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"

[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

### 通过快照播种的全栈

每个实例都从你主机现有数据库卷的副本开始，然后各自独立地分叉演进。

```toml
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

### 为每个实例提供干净数据库的测试运行器

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
service = "mongodb"
mount = "/data/db"
```
