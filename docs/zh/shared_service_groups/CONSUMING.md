# 使用共享服务组

消费者 Coast 通过在消费者的 `Coastfile` 中使用一行标志，按服务选择加入其项目由 SSG 拥有的服务。在 Coast 内部，应用容器仍然看到 `postgres:5432`；守护进程的路由层通过一个稳定的虚拟端口将该流量重定向到项目的 `<project>-ssg` 外层 DinD。

`from_group = true` 引用的 SSG **始终是消费者项目自己的 SSG**。不存在跨项目共享。如果消费者的 `[coast].name` 是 `cg`，则 `from_group = true` 会相对于 `cg-ssg` 的 `Coastfile.shared_service_groups` 进行解析。

## 语法

添加一个带有 `from_group = true` 的 `[shared_services.<name>]` 块:

```toml
# Consumer Coastfile
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true

# Optional per-project overrides:
inject = "env:DATABASE_URL"
# auto_create_db = true       # overrides the SSG service's default
```

TOML 键（本例中为 `postgres`）必须与项目的 `Coastfile.shared_service_groups` 中声明的服务名称匹配。

## 禁止字段

当使用 `from_group = true` 时，以下字段会在解析时被拒绝:

- `image`
- `ports`
- `env`
- `volumes`

这些都定义在 SSG 侧。如果其中任何一个与 `from_group = true` 同时出现，`coast build` 会失败并显示:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports, env, volumes.
```

## 允许的覆盖项

每个消费者仍然可以合法使用两个字段:

- `inject` -- 暴露连接字符串所使用的环境变量或文件路径。不同的消费者项目可以使用不同的环境变量名来暴露相同格式的值。
- `auto_create_db` -- Coast 是否应在 `coast run` 时在该服务内创建一个按实例划分的数据库。此值会覆盖 SSG 服务自身的 `auto_create_db` 值。

## 冲突检测

在单个 Coastfile 中，两个名称相同的 `[shared_services.<name>]` 块会在解析时被拒绝。该规则保持不变。

如果某个带有 `from_group = true` 的块引用了一个未在项目的 `Coastfile.shared_service_groups` 中声明的名称，则会在 `coast build` 时失败:

```text
error: shared service 'postgres' has from_group = true but no service named 'postgres' is declared in Coastfile.shared_service_groups for project 'my-app'.
```

这是拼写错误检查。不存在单独的运行时“漂移”检查——消费者与 SSG 之间的结构不匹配会在构建时检查中体现，而运行时的任何进一步不匹配都会自然地从应用视角表现为连接错误。

## 自动启动

在消费者上执行 `coast run` 时，如果项目的 SSG 尚未运行，则会自动启动:

- SSG 构建已存在，但容器未运行 -> 守护进程执行等价于 `coast ssg start` 的操作（如果容器从未创建过，则执行 `run`），并受项目 SSG 互斥锁保护。
- 完全不存在 SSG 构建 -> 硬错误:

  ```text
  Project 'my-app' references shared service 'postgres' from the Shared Service Group, but no SSG build exists. Run `coast ssg build` in the directory containing your Coastfile.shared_service_groups.
  ```

- SSG 已在运行 -> 无操作，`coast run` 立即继续。

进度事件 `SsgStarting` 和 `SsgStarted` 会在运行流中触发，以便 [Coastguard](../concepts_and_terminology/COASTGUARD.md) 能将启动归因到消费者项目。

## 路由如何工作

在消费者 Coast 内部，应用容器通过三个组成部分将 `postgres:5432` 解析到项目的 SSG:

1. **别名 IP + `extra_hosts`** 将 `postgres -> <docker0 alias IP>` 添加到消费者的内部 compose 中，因此对 `postgres` 的 DNS 查询会成功。
2. **DinD 内部 socat** 监听 `<alias>:5432` 并转发到 `host.docker.internal:<virtual_port>`。该虚拟端口对于 `(project, service, container_port)` 是稳定的——当 SSG 重建时它不会改变。
3. **主机 socat** 在 `<virtual_port>` 上监听，并转发到 `127.0.0.1:<dynamic>`，其中 `<dynamic>` 是 SSG 容器当前发布的端口。主机 socat 会在 SSG 重建时更新；消费者的 DinD 内部 socat 则无需变更。

应用代码和 compose DNS 都不需要改变。将项目从内联 Postgres 迁移到 SSG Postgres 只需要对 Coastfile 做一个小改动（移除 `image`/`ports`/`env`，添加 `from_group = true`）然后重新构建。

有关逐跳完整说明、端口概念和设计原理，请参阅 [Routing](ROUTING.md)。

## `auto_create_db`

在 SSG 的 Postgres 或 MySQL 服务上设置 `auto_create_db = true`，会使守护进程为每个运行的消费者 Coast 在该服务内创建一个 `{instance}_{project}` 数据库。数据库名称与内联 `[shared_services]` 模式生成的名称一致，因此 `inject` URL 与 `auto_create_db` 创建的数据库保持一致。

创建是幂等的。对数据库已存在的实例重新运行 `coast run` 不会执行任何操作。底层 SQL 与内联路径完全相同，因此无论项目使用哪种模式，DDL 输出都逐字节一致。

消费者可以覆盖 SSG 服务的 `auto_create_db` 值:

```toml
# SSG: auto_create_db = true, but this project doesn't want per-instance DBs.
[shared_services.postgres]
from_group = true
auto_create_db = false
```

## `inject`

`inject` 向应用容器暴露连接字符串。格式与 [Secrets](../coastfiles/SECRETS.md) 相同:`"env:NAME"` 创建一个环境变量，`"file:/path"` 在消费者的 coast 容器内写入一个文件，并将其以只读方式 bind-mount 到每个未被 stub 的内部 compose 服务中。

解析后的字符串使用规范服务名和规范端口，而不是动态主机端口。这种不变性正是关键——无论 SSG 恰好发布到哪个动态端口，应用容器始终看到 `postgres://coast:coast@postgres:5432/{db}`。

`env:NAME` 和 `file:/path` 都已完整实现。

这里的 `inject` 是**消费者侧**的 secret 管道:该值在 `coast build` 时根据规范的 SSG 元数据计算，并注入到消费者的 coast DinD 中。它独立于 **SSG 侧** 的 `[secrets.*]` 管道（见 [Secrets](SECRETS.md)），后者用于提取供 SSG 的*自身*服务使用的值。

## 远程 Coast

远程 Coast（使用 `coast assign --remote ...` 创建）通过反向 SSH 隧道访问本地 SSG。本地守护进程会从远程机器回连到本地虚拟端口并启动 `ssh -N -R <vport>:localhost:<vport>`；在远程 DinD 内部，`extra_hosts: postgres: host-gateway` 会将 `postgres` 解析为远程的 host-gateway IP，而 SSH 隧道会在另一端通过相同的虚拟端口号连接到本地 SSG。

隧道两端使用的都是**虚拟**端口，而不是动态端口。这意味着在本地重建 SSG 永远不会使远程隧道失效。

隧道按 `(project, remote_host, service, container_port)` 合并——同一远程主机上同一项目的多个消费者实例共享一个 `ssh -R` 进程。移除一个消费者不会拆除隧道；只有移除最后一个消费者时才会拆除。

实际影响:

- 当某个远程影子 Coast 正在使用该 SSG 时，`coast ssg stop` / `rm` 会拒绝执行。守护进程会列出阻塞的影子，以便你知道是谁在使用该 SSG。
- `coast ssg stop --force`（或 `rm --force`）会先拆除共享的 `ssh -R`，然后继续执行。当你接受远程消费者将失去连接时，请使用此选项。

完整的远程隧道架构请参阅 [Routing](ROUTING.md)，更广泛的远程机器设置请参阅 [Remote Coasts](../remote_coasts/README.md)。

## 另请参阅

- [Routing](ROUTING.md) -- 规范 / 动态 / 虚拟端口概念以及完整路由链
- [Secrets](SECRETS.md) -- 面向服务端凭据的 SSG 原生 `[secrets.*]`（与消费者侧 `inject` 正交）
- [Coastfile: Shared Services](../coastfiles/SHARED_SERVICES.md) -- 完整的 `[shared_services.*]` 模式，包括 `from_group = true`
- [Lifecycle](LIFECYCLE.md) -- `coast run` 在幕后执行的操作，包括自动启动
- [Checkout](CHECKOUT.md) -- 面向临时工具的主机侧规范端口绑定
- [Volumes](VOLUMES.md) -- 挂载和权限；当你重建 SSG 且新的 Postgres 镜像更改数据目录所有权时尤为相关
