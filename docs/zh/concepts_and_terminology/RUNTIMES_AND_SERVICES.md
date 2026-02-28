# 运行时与服务

一个 Coast 运行在一个容器运行时之中——一个外层容器，托管其自身的 Docker（或 Podman）守护进程。你的项目服务运行在该内层守护进程中，与其他 Coast 实例完全隔离。目前，**DinD（Docker-in-Docker）是唯一经过生产环境测试的运行时。** 目前我们建议你继续使用 DinD，直到 Podman 和 Sysbox 支持经过充分测试。

## 运行时

Coastfile 中的 `runtime` 字段用于选择为 Coast 提供支撑的容器运行时。默认值是 `dind`，你也可以完全省略它:

```toml
[coast]
name = "my-app"
runtime = "dind"
```

可接受三个值:`dind`、`sysbox` 和 `podman`。在实际中，只有 DinD 已接入守护进程并进行了端到端测试。

### DinD（Docker-in-Docker）

目前默认且唯一建议使用的运行时。Coast 会基于 `docker:dind` 镜像创建一个容器，并启用 `--privileged` 模式。在该容器内部，会启动完整的 Docker 守护进程，你的 `docker-compose.yml` 服务将作为嵌套容器运行。

DinD 已完全集成:

- 镜像会在宿主机上预缓存，并在 `coast run` 时加载到内层守护进程中
- 每个实例的镜像会在宿主机上构建，并通过 `docker save | docker load` 管道传入
- 内层守护进程的状态会持久化在一个具名卷（`coast-dind--{project}--{instance}`）的 `/var/lib/docker` 中，因此后续运行可完全跳过镜像加载
- 端口会直接从 DinD 容器发布到宿主机
- Compose 覆盖、共享服务网络桥接、secret 注入以及卷策略都可用

### Sysbox（未来）

Sysbox 是一个仅限 Linux 的 OCI 运行时，提供无需 `--privileged` 的无 root（rootless）容器。它会使用 `--runtime=sysbox-runc` 来替代特权模式，这是更好的安全姿态。该 trait 实现已存在于代码库中，但尚未连接到守护进程。在 macOS 上不可用。

### Podman（未来）

Podman 将用运行在 `quay.io/podman/stable` 内的 Podman 守护进程替换内层 Docker 守护进程，并使用 `podman-compose` 替代 `docker compose`。该 trait 实现已存在，但尚未连接到守护进程。

当 Sysbox 和 Podman 支持趋于稳定后，本页面将更新。现在请将 `runtime` 保持为 `dind` 或省略它。

## Docker-in-Docker 架构

每个 Coast 都是一个嵌套容器。宿主机 Docker 守护进程管理外层 DinD 容器，而其内部的 Docker 守护进程管理你的 compose 服务。

```text
Host machine
│
├── Docker daemon (host)
│   │
│   ├── coast container: dev-1 (docker:dind, --privileged)
│   │   │
│   │   ├── Inner Docker daemon
│   │   │   ├── web        (your app, :3000)
│   │   │   ├── postgres   (database, :5432)
│   │   │   └── redis      (cache, :6379)
│   │   │
│   │   ├── /workspace          ← bind mount of your project root
│   │   ├── /image-cache        ← read-only mount of ~/.coast/image-cache/
│   │   ├── /coast-artifact     ← read-only mount of the build artifact
│   │   ├── /coast-override     ← generated compose overrides
│   │   └── /var/lib/docker     ← named volume (inner daemon state)
│   │
│   ├── coast container: dev-2 (docker:dind, --privileged)
│   │   └── (same structure, fully isolated)
│   │
│   └── shared postgres (host-level, bridge network)
│
└── ~/.coast/
    ├── image-cache/    ← OCI tarballs shared across all projects
    └── state.db        ← instance metadata
```

当 `coast run` 创建一个实例时，它会:

1. 在宿主机守护进程上创建并启动 DinD 容器
2. 在容器内轮询 `docker info`，直到内层守护进程就绪（最多 120 秒）
3. 检查内层守护进程已拥有的镜像（来自持久化的 `/var/lib/docker` 卷），并从缓存中加载缺失的 tarball
4. 通过 `docker save | docker load` 将在宿主机上构建的每实例镜像以管道方式传入
5. 将 `/host-project` 绑定到 `/workspace`，使 compose 服务能够看到你的源代码
6. 在容器内运行 `docker compose up -d`，并等待所有服务处于运行中或健康状态

持久化的 `/var/lib/docker` 卷是关键优化点。在一次全新的 `coast run` 中，将镜像加载到内层守护进程可能需要 20 秒以上。在后续运行中（即使 `coast rm` 后再重新运行），内层守护进程已经缓存了这些镜像，启动时间会降到 10 秒以内。

## 服务

服务是运行在你的 Coast 内的容器（或在 [bare services](BARE_SERVICES.md) 的情况下为进程）。对于基于 compose 的 Coast，这些服务就是在你的 `docker-compose.yml` 中定义的服务。

![Coastguard 中的 Services 标签页](../../assets/coastguard-services.png)
*Coastguard 的 Services 标签页，显示 compose 服务、它们的状态、镜像以及端口映射。*

Coastguard 的 Services 标签页会显示在某个 Coast 实例内运行的每个服务:

- **Service** — compose 服务名称（例如 `web`、`backend`、`redis`）。点击可查看该容器的详细 inspect 数据、日志和统计信息。
- **Status** — 服务是运行中、已停止，还是处于错误状态。
- **Image** — 服务所基于构建的 Docker 镜像。
- **Ports** — 原始的 compose 端口映射，以及由 coast 管理的 [canonical/dynamic ports](PORTS.md)。动态端口始终可访问；规范端口只会路由到被 [checked-out](CHECKOUT.md) 的实例。

你可以选择多个服务，并在工具栏中批量停止、启动、重启或移除它们。

配置为 [shared services](SHARED_SERVICES.md) 的服务运行在宿主机守护进程上，而不是在 Coast 内，因此不会出现在此列表中。它们有自己的标签页。

## `coast ps`

Services 标签页对应的 CLI 命令是 `coast ps`:

```bash
coast ps dev-1
```

```text
Services in coast instance 'dev-1':
  NAME                      STATUS               PORTS
  backend                   running              0.0.0.0:8080->8080/tcp, 0.0.0.0:40000->40000/tcp
  mailhog                   running              0.0.0.0:1025->1025/tcp, 0.0.0.0:8025->8025/tcp
  reach-web                 running              0.0.0.0:4000->4000/tcp
  test-redis                running              0.0.0.0:6380->6379/tcp
  web                       running              0.0.0.0:3000->3000/tcp
```

在底层，守护进程会在 DinD 容器内执行 `docker compose ps --format json` 并解析 JSON 输出。结果在返回前会经过若干过滤:

- **Shared services** 会被剔除——它们运行在宿主机上，而不是在 Coast 内。
- **一次性任务**（没有端口的服务）在成功退出后会被隐藏。如果它们失败，则会显示出来以便你调查。
- **缺失的服务** ——如果一个应该存在的长期运行服务没有出现在输出中，它会被以 `down` 状态补充进去，以便你知道出了问题。

如需更深入的检查，请使用 `coast logs` 追踪服务输出，并使用 [`coast exec`](EXEC_AND_DOCKER.md) 进入 Coast 容器获取一个 shell。关于日志流式传输以及 MCP 权衡的完整细节，请参见 [Logs](LOGS.md)。

```bash
coast logs dev-1 --service web --tail 100
coast exec dev-1
```
