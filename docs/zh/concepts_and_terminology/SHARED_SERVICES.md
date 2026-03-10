# 共享服务

共享服务是在你的主机 Docker 守护进程上运行的数据库和基础设施容器（Postgres、Redis、MongoDB 等），而不是在 Coast 内部运行。Coast 实例通过桥接网络连接到它们，因此每个 Coast 都会与同一主机卷上的同一服务通信。

![Coastguard 中的共享服务](../../assets/coastguard-shared-services.png)
*Coastguard 共享服务标签页，显示由主机管理的 Postgres、Redis 和 MongoDB。*

## 工作原理

当你在 Coastfile 中声明共享服务时，Coast 会在主机守护进程上启动它，并将其从每个 Coast 容器内部运行的 compose 栈中移除。随后会将 Coasts 配置为把服务名流量路由回共享容器，同时在 Coast 内保留该服务的容器侧端口。

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

由于共享服务会重用你现有的主机卷，因此你通过本地运行 `docker-compose up` 已经拥有的任何数据都可以立即供你的 Coasts 使用。

当你使用映射端口时，这一区别尤为重要:

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- 在主机上，共享服务发布在 `localhost:5433`。
- 在每个 Coast 内部，应用容器仍然连接到 `postgis:5432`。
- 像 `5432` 这样的裸整数是恒等映射 `"5432:5432"` 的简写。

## 何时使用共享服务

- 你的项目具有连接到本地数据库的 MCP 集成 —— 共享服务可让它们继续工作，而无需动态端口发现。如果你在主机上用工具已在使用的相同端口发布共享服务（例如 `ports = [5432]`），这些工具无需修改即可继续工作。如果你将其发布到不同的主机端口（例如 `"5433:5432"`），则主机侧工具应使用该主机端口，而 Coasts 继续使用容器端口。
- 你希望 Coast 实例更轻量，因为它们不需要运行自己的数据库容器。
- 你不需要 Coast 实例之间的数据隔离（每个实例看到的都是相同的数据）。
- 你正在主机上运行编码代理（见 [Filesystem](FILESYSTEM.md)），并希望它们无需通过 [`coast exec`](EXEC_AND_DOCKER.md) 路由即可访问数据库状态。使用共享服务时，代理现有的数据库工具和 MCP 可保持不变地工作。

如果你确实需要隔离，请参阅 [卷拓扑](VOLUMES.md) 页面了解替代方案。

## 卷歧义警告

Docker 卷名称并不总是在全局范围内唯一。如果你从多个不同项目运行 `docker-compose up`，Coast 附加到共享服务的主机卷可能不是你期望的那些。

在使用共享服务启动 Coasts 之前，请确保你上一次运行的 `docker-compose up` 来自你打算与 Coasts 一起使用的项目。这样可以确保主机卷与你的 Coastfile 期望一致。

## 故障排除

如果你的共享服务似乎指向了错误的主机卷:

1. 打开 [Coastguard](COASTGUARD.md) UI（`coast ui`）。
2. 导航到 **Shared Services** 标签页。
3. 选择受影响的服务并点击 **Remove**。
4. 点击 **Refresh Shared Services** 以根据你当前的 Coastfile 配置重新创建它们。

这会拆除并重新创建共享服务容器，并将它们重新附加到正确的主机卷。
