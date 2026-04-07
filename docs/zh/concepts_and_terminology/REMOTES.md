# 远程 Coast

远程 coast 会在远程机器而不是你的笔记本上运行你的服务。CLI 和 UI 的体验与本地 coast 完全相同——`coast run`、`coast assign`、`coast exec`、`coast ps` 和 `coast checkout` 的工作方式都一样。守护进程会检测该实例是远程实例，并通过 SSH 隧道将操作路由到远程主机上的 `coast-service`。

## 本地与远程

| | 本地 Coast | 远程 Coast |
|---|---|---|
| DinD 容器 | 在你的机器上运行 | 在远程机器上运行 |
| Compose 服务 | 在本地 DinD 内部 | 在远程 DinD 内部 |
| 文件编辑 | 直接绑定挂载 | shell coast（本地）+ rsync/mutagen 同步 |
| 端口访问 | `socat` 转发器 | SSH `-L` 隧道 + `socat` 转发器 |
| 共享服务 | Bridge 网络 | SSH `-R` 反向隧道 |
| 构建架构 | 你的机器架构 | 远程机器架构 |

## 工作原理

每个远程 coast 会创建两个容器:

1. 你本地机器上的一个 **shell coast**。这是一个轻量级 Docker 容器（`sleep infinity`），具有与普通 coast 相同的绑定挂载（`/host-project`、`/workspace`）。它的存在是为了让宿主机代理能够编辑文件，并将其同步到远程端。

2. 远程机器上的一个 **remote coast**，由 `coast-service` 管理。它运行实际的 DinD 容器以及你的 compose 服务，并使用动态端口。

守护进程通过 SSH 隧道将它们连接起来:

- **正向隧道**（`ssh -L`）:将每个本地动态端口映射到对应的远程动态端口，因此 `localhost:{dynamic}` 可以访问远程服务。
- **反向隧道**（`ssh -R`）:将本地的[共享服务](SHARED_SERVICES.md)（Postgres、Redis）暴露给远程 DinD 容器。

## 注册远程端

远程端通过守护进程注册，并存储在 `state.db` 中:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/coast_key
coast remote test my-vm
coast remote ls
coast remote rm my-vm
```

连接详情（主机、用户、端口、SSH 密钥）保存在守护进程的数据库中，而不是你的 Coastfile 中。Coastfile 仅通过 `[remote]` 部分声明同步偏好。

## 远程构建

构建发生在远程机器上，因此镜像会使用远程机器的原生架构。ARM Mac 可以在 x86_64 远程主机上构建 x86_64 镜像，而无需交叉编译。

构建完成后，产物会被传回你的本地机器以供复用。如果另一个远程端具有相同的架构，则该预构建产物可以直接部署而无需重新构建。有关构建产物结构的更多信息，请参阅[构建](BUILDS.md)。

## 文件同步

远程 coast 使用 rsync 进行初始批量传输，并使用 mutagen 进行持续的实时同步。这两个工具都运行在 coast 容器内（shell coast 和 coast-service 镜像），而不是在你的宿主机上运行。有关同步配置的详细信息，请参阅[远程 Coast](../remote_coasts/README.md)指南。

## 磁盘管理

远程机器会积累 Docker 卷、工作区目录和镜像 tarball。当 `coast rm` 删除远程实例时，所有相关资源都会被清理。对于因失败操作而遗留的孤立资源，请使用 `coast remote prune`。

## 设置

有关完整的设置说明，包括宿主机要求、coast-service 部署和 Coastfile 配置，请参阅[远程 Coast](../remote_coasts/README.md)指南。
