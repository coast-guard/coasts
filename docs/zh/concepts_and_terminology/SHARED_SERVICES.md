# 共享服务

共享服务是数据库和基础设施容器（Postgres、Redis、MongoDB 等），它们运行在你的主机 Docker 守护进程上，而不是在某个 Coast 内部。Coast 实例通过桥接网络连接到这些服务，因此每个 Coast 都会连接到同一主机卷上的同一服务。

![Coastguard 中的共享服务](../../assets/coastguard-shared-services.png)
*Coastguard 的共享服务标签页，显示由主机管理的 Postgres、Redis 和 MongoDB。*

## 工作原理

当你在 Coastfile 中声明共享服务时，Coast 会在主机守护进程上启动它，并将其从每个 Coast 容器内部运行的 compose 堆栈中移除。随后，Coast 会被配置为将发往服务名的流量路由回共享容器，同时在 Coast 内部保留该服务在容器侧的端口。

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

由于共享服务会复用你现有的主机卷，任何你之前通过在本地运行 `docker-compose up` 已有的数据都会立即对你的 Coasts 可用。

当你使用端口映射时，这一区别尤其重要:

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- 在主机上，共享服务会发布在 `localhost:5433`。
- 在每个 Coast 内部，应用容器仍然连接到 `postgis:5432`。
- 像 `5432` 这样的裸整数是恒等映射 `"5432:5432"` 的简写。

## 何时使用共享服务

- 你的项目有连接到本地数据库的 MCP 集成——共享服务可让它们继续工作，而无需动态端口发现。如果你将共享服务发布到你的工具已在使用的同一主机端口（例如 `ports = [5432]`），这些工具无需更改即可继续工作。如果你将其发布到不同的主机端口（例如 `"5433:5432"`），则主机侧工具应使用该主机端口，而 Coasts 继续使用容器端口。
- 你希望 Coast 实例更轻量，因为它们不需要运行各自的数据库容器。
- 你不需要 Coast 实例之间的数据隔离（每个实例看到的都是相同的数据）。
- 你在主机上运行编码代理（见 [Filesystem](FILESYSTEM.md)），并希望它们无需通过 [`coast exec`](EXEC_AND_DOCKER.md) 路由就能访问数据库状态。使用共享服务后，代理现有的数据库工具和 MCP 都可以保持不变地继续工作。

当你确实需要隔离时，请参阅 [Volume Topology](VOLUMES.md) 页面了解替代方案。

## 卷歧义警告

Docker 卷名称并不总是在全局范围内唯一。如果你从多个不同项目运行 `docker-compose up`，那么 Coast 附加到共享服务的主机卷可能并不是你所期望的那些。

在使用共享服务启动 Coasts 之前，请确保你上一次运行的 `docker-compose up` 来自你打算与 Coasts 一起使用的项目。这样可以确保主机卷与你的 Coastfile 预期一致。

## 故障排查

如果你的共享服务似乎指向了错误的主机卷:

1. 打开 [Coastguard](COASTGUARD.md) UI（`coast ui`）。
2. 导航到 **Shared Services** 标签页。
3. 选择受影响的服务并点击 **Remove**。
4. 点击 **Refresh Shared Services**，根据你当前的 Coastfile 配置重新创建它们。

这会拆除并重新创建共享服务容器，并将它们重新附加到正确的主机卷。

## 共享服务与远程 Coasts

运行[远程 coasts](REMOTES.md)时，共享服务仍然在你的本地机器上运行。守护进程会建立 SSH 反向隧道（`ssh -R`），使远程 DinD 容器能够通过 `host.docker.internal` 访问它们。这样可以让你的本地数据库继续与远程实例共享。远程主机的 sshd 必须启用 `GatewayPorts clientspecified`，以便反向隧道能够正确绑定。
