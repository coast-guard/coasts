# 共享服务

共享服务是数据库和基础设施容器（Postgres、Redis、MongoDB 等），它们运行在你的主机 Docker 守护进程上，而不是在某个 Coast 内部。Coast 实例通过桥接网络连接到它们，因此每个 Coast 都会与同一主机卷上的同一服务通信。

![Shared services in Coastguard](../../assets/coastguard-shared-services.png)
*Coastguard 的共享服务选项卡，显示由主机管理的 Postgres、Redis 和 MongoDB。*

## 它们如何工作

当你在 Coastfile 中声明共享服务时，Coast 会在主机守护进程上启动它，并将其从每个 Coast 容器内运行的 compose 堆栈中移除。随后会配置各个 Coast，将它们的连接路由回主机。

```text
Host Docker daemon
  |
  +--> postgres (host volume: infra_postgres_data)
  +--> redis    (host volume: infra_redis_data)
  +--> mongodb  (host volume: infra_mongodb_data)
  |
  +--> Coast: dev-1  --bridge network--> host postgres, redis, mongodb
  +--> Coast: dev-2  --bridge network--> host postgres, redis, mongodb
```

由于共享服务会复用你现有的主机卷，你之前在本地运行 `docker-compose up` 所产生的任何数据，都能立即在你的各个 Coast 中使用。

## 何时使用共享服务

- 你的项目有连接本地数据库的 MCP 集成——共享服务让它们无需重新配置也能继续工作。主机上的数据库 MCP 若连接 `localhost:5432`，仍然可以正常工作，因为共享的 Postgres 在主机上同样使用该端口。无需动态端口发现，无需重新配置 MCP。更多信息请参见 [MCP Servers](MCP_SERVERS.md)。
- 你希望 Coast 实例更轻量，因为它们不需要运行自己的数据库容器。
- 你不需要在 Coast 实例之间进行数据隔离（每个实例看到的都是同一份数据）。
- 你在主机上运行编码代理（参见 [Filesystem](FILESYSTEM.md)），并希望它们在不通过 [`coast exec`](EXEC_AND_DOCKER.md) 路由的情况下访问数据库状态。使用共享服务时，代理现有的数据库工具和 MCP 可保持不变并直接工作。

当你确实需要隔离时，请参见 [Volume Topology](VOLUMES.md) 页面了解替代方案。

## 卷歧义警告

Docker 卷名称并不总是全局唯一。如果你从多个不同项目运行 `docker-compose up`，Coast 连接到共享服务的主机卷可能并不是你期望的那个。

在使用共享服务启动 Coasts 之前，请确保你最后一次运行的 `docker-compose up` 来自你打算与 Coasts 一起使用的项目。这可以确保主机卷与你的 Coastfile 预期一致。

## 故障排除

如果你的共享服务看起来指向了错误的主机卷:

1. 打开 [Coastguard](COASTGUARD.md) UI（`coast ui`）。
2. 进入 **Shared Services** 选项卡。
3. 选择受影响的服务并点击 **Remove**。
4. 点击 **Refresh Shared Services**，根据你当前的 Coastfile 配置重新创建它们。

这会拆除并重新创建共享服务容器，将它们重新连接到正确的主机卷。
