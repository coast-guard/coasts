# 卷拓扑

Coast 提供三种卷策略，用于控制数据密集型服务（数据库、缓存等）如何在各个 Coast 实例之间存储和共享其数据。选择合适的策略取决于你需要多少隔离性，以及你能容忍多少开销。

## 共享服务

[共享服务](SHARED_SERVICES.md) 运行在你的主机 Docker 守护进程上，在任何 Coast 容器之外。像 Postgres、MongoDB 和 Redis 这样的服务会留在主机上，Coast 实例通过桥接网络将调用路由回主机。

```text
Host machine
  |
  +--> Postgres (host daemon, existing volume)
  +--> Redis (host daemon, existing volume)
  |
  +--> Coast: dev-1  --connects to--> host Postgres, host Redis
  +--> Coast: dev-2  --connects to--> host Postgres, host Redis
```

实例之间没有数据隔离——每个 Coast 都在与同一个数据库通信。作为回报，你将获得:

- 更轻量的 Coast 实例，因为它们不运行自己的数据库容器。
- 直接复用你现有的主机卷，因此你已有的任何数据都能立即可用。
- 连接到本地数据库的 MCP 集成开箱即用，继续正常工作。

这在你的 [Coastfile](COASTFILE_TYPES.md) 中通过 `[shared_services]` 配置。

## 共享卷

共享卷会挂载一个在所有 Coast 实例之间共享的单个 Docker 卷。服务本身（Postgres、Redis 等）在每个 Coast 容器内运行，但它们都会对同一个底层卷进行读写。

```text
Coast: dev-1  --mounts--> shared volume "my-project-postgres"
Coast: dev-2  --mounts--> shared volume "my-project-postgres"
```

这会将你的 Coast 数据与主机上的内容隔离开来，但实例之间仍然共享数据。当你希望与主机开发环境彻底分离，同时又不想承担为每个实例创建独立卷的开销时，这会很有用。

```toml
[volumes.postgres_data]
strategy = "shared"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

## 隔离卷

隔离卷会为每个 Coast 实例提供其各自独立的卷。实例之间以及与主机之间都不共享任何数据。每个实例都从空开始（或从快照开始——见下文），并各自独立地演进。

```text
Coast: dev-1  --mounts--> volume "dev-1-postgres"
Coast: dev-2  --mounts--> volume "dev-2-postgres"
```

对于集成测试密集、需要在并行环境之间实现真正卷隔离的项目来说，这是最佳选择。代价是启动更慢、Coast 构建更大，因为每个实例都维护自己的一份数据副本。

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

## 快照

共享与隔离两种策略默认都从空卷开始。如果你希望实例以现有主机卷的副本作为起点，将 `snapshot_source` 设置为要从中复制的 Docker 卷名称:

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

快照会在[构建时](BUILDS.md)获取。创建之后，每个实例的卷会各自独立地分叉——更改不会回传到源卷，也不会传播到其他实例。

Coast 尚不支持运行时快照（例如，从正在运行的实例对卷进行快照）。这计划在未来版本中提供。
