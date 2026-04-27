# SSG 生命周期

每个项目的 SSG 都是其各自独立的外层 Docker-in-Docker 容器，名称为 `<project>-ssg`（例如 `cg-ssg`）。生命周期相关动词会作用于拥有当前工作目录 `Coastfile` 的那个项目的 SSG（或通过 `--working-dir` 指定的项目）。每个会修改状态的命令都会通过守护进程中的项目级互斥锁串行化，因此针对同一项目并发执行两个 `coast ssg run` / `coast ssg stop` 调用时会排队而不是竞争——但两个不同项目可以并行修改各自的 SSG。

## 状态机

```text
                     coast ssg build           coast ssg run
(no build)   -->  built     -->     created    -->     running
                                                          |
                                                   coast ssg stop
                                                          v
                                                       stopped
                                                          |
                                                  coast ssg start
                                                          v
                                                       running
                                                          |
                                                   coast ssg rm
                                                          v
                                                      (removed)
```

- `coast ssg build` 不会创建容器。它会在磁盘上的 `~/.coast/ssg/<project>/builds/<id>/` 下生成一个构件，并且（当声明了 `[secrets.*]` 时）会将密钥值提取到 keystore 中。
- `coast ssg run` 会创建 `<project>-ssg` DinD，分配动态主机端口，将任何已声明的密钥实例化到每次运行专属的 `compose.override.yml` 中，并启动内部 compose 栈。
- `coast ssg stop` 会停止外层 DinD，但保留容器、动态端口记录以及每项目虚拟端口，因此 `start` 会很快。
- `coast ssg start` 会重新启动 SSG 并重新实例化密钥（因此在 stop 和 start 之间执行的 `coast ssg secrets clear` 会生效）。
- `coast ssg rm` 会删除外层 DinD 容器。配合 `--with-data` 时，它还会删除内部命名卷（永远不会触碰主机 bind-mount 的内容）。keystore 永远不会被 `rm` 清除——只有 `coast ssg secrets clear` 会这样做。
- `coast ssg restart` 是 `stop` + `start` 的便捷封装。

## 命令

### `coast ssg run`

如果 `<project>-ssg` DinD 不存在，则创建它并启动其内部服务。为每个已声明的服务分配一个动态主机端口，并将它们发布到外层 DinD 上。将这些映射写入状态 DB，以便端口分配器不会重复使用它们。

```bash
coast ssg run
```

通过与 `coast ssg build` 相同的 `BuildProgressEvent` 通道流式输出进度事件。默认计划有 7 个步骤:

1. 准备 SSG
2. 创建 SSG 容器
3. 启动 SSG 容器
4. 等待内部守护进程
5. 加载缓存镜像
6. 实例化密钥（没有 `[secrets]` 块时静默；否则按密钥逐项输出）
7. 启动内部服务

**自动启动**。当消费者 Coast 引用了某个 SSG 服务时，`coast run` 会在该 SSG 尚未运行时自动启动它。你当然也可以始终显式运行 `coast ssg run`，但通常没有必要。参见 [Consuming -> Auto-start](CONSUMING.md#auto-start)。

### `coast ssg start`

启动一个之前已停止的 SSG。要求存在一个已有的 `<project>-ssg` 容器（即此前执行过 `coast ssg run`）。会从 keystore 重新实例化密钥，以使 stop 之后的任何更改生效，然后为所有在 stop 之前已 checkout 的规范端口重新生成主机侧 checkout socat。

```bash
coast ssg start
```

### `coast ssg stop`

停止外层 DinD 容器。内部 compose 栈也会随之关闭。容器、动态端口分配以及每项目虚拟端口记录都会被保留，因此下一次 `start` 会很快。

```bash
coast ssg stop
coast ssg stop --force
```

主机侧 checkout socat 会被终止，但它们在状态 DB 中的记录会保留。下一次 `coast ssg start` 或 `coast ssg run` 会重新生成它们。参见 [Checkout](CHECKOUT.md)。

**远程消费者门控。** 当任何远程 shadow Coast（通过 `coast assign --remote ...` 创建）当前正在消费该 SSG 时，守护进程会拒绝停止该 SSG。传入 `--force` 可强制拆除反向 SSH 隧道并继续。参见 [Consuming -> Remote Coasts](CONSUMING.md#remote-coasts)。

### `coast ssg restart`

等价于 `stop` + `start`。保留容器和动态端口映射。

```bash
coast ssg restart
```

### `coast ssg rm`

删除外层 DinD 容器。默认情况下会保留内部命名卷（Postgres WAL 等），因此你的数据可以在 `rm` / `run` 周期之间保留。主机 bind-mount 的内容永远不会被触碰。

```bash
coast ssg rm                    # 保留命名卷；保留 keystore
coast ssg rm --with-data        # 也删除命名卷；仍保留 keystore
coast ssg rm --force            # 即使存在远程消费者也继续
```

- `--with-data` 会在删除 DinD 本身之前删除所有内部命名卷。当你想要一个全新的数据库时使用它。
- `--force` 即使在远程 shadow Coasts 引用了该 SSG 时也会继续。语义与 `stop --force` 相同。
- `rm` 会清除 `ssg_port_checkouts` 记录（会破坏规范端口的主机绑定）。

keystore——SSG 原生密钥的存放位置（`coast_image = "ssg:<project>"`）——**不会**受到 `rm` 或 `rm --with-data` 的影响。要清除 SSG 密钥，请使用 `coast ssg secrets clear`（参见 [Secrets](SECRETS.md)）。

### `coast ssg ps`

显示当前项目 SSG 的服务状态。读取 `manifest.json` 以获取已构建的配置，然后检查实时状态 DB 以获取正在运行的容器元数据。

```bash
coast ssg ps
```

成功执行 `run` 之后的输出:

```text
SSG build: b455787d95cfdeb_20260420061903  (project: cg, running)

  SERVICE              IMAGE                          PORT       STATUS
  postgres             postgres:16                    5432       running
  redis                redis:7-alpine                 6379       running
```

### `coast ssg ports`

显示每个服务的 canonical / dynamic / virtual 端口映射；当某个服务存在处于活动状态的主机侧规范端口 socat 时，会带有 `(checked out)` 标注。virtual 端口才是消费者实际连接的端口。详情参见 [Routing](ROUTING.md)。

```bash
coast ssg ports

#   SERVICE              CANONICAL       DYNAMIC         VIRTUAL    STATUS
#   postgres             5432            54201           42000      (checked out)
#   redis                6379            54202           42001
```

### `coast ssg logs`

从外层 DinD 容器或指定的内部服务流式输出日志。

```bash
coast ssg logs --tail 100
coast ssg logs --service postgres --tail 50
coast ssg logs --service postgres --follow
```

- `--service <name>` 通过 compose 键名指定一个内部服务；如果不提供，则得到外层 DinD 的 stdout。
- `--tail N` 限制历史行数（默认 200）。
- `--follow` / `-f` 会在新行到达时持续流式输出，直到 `Ctrl+C`。

### `coast ssg exec`

在外层 DinD 或某个内部服务中执行命令。

```bash
coast ssg exec -- sh
coast ssg exec --service postgres -- psql -U coast -l
```

- 不带 `--service` 时，命令在外层 `<project>-ssg` 容器中运行。
- 带 `--service <name>` 时，命令会通过 `docker compose exec -T` 在对应 compose 服务内运行。
- `--` 之后的所有内容都会原样传递给底层 `docker exec`，包括标志位。

### `coast ssg ls`

列出守护进程已知的所有 SSG，涵盖所有项目。这是唯一一个不会根据 cwd 解析项目的动词；它会返回守护进程 SSG 状态中每个条目的记录。

```bash
coast ssg ls

#   PROJECT     STATUS     BUILD                                       SERVICES   CREATED
#   cg          running    b455787d95cfdeb_20260420061903               2          2026-04-20T06:19:03Z
#   filemap     stopped    b9b93fdb41b21337_20260418123012               3          2026-04-18T12:30:12Z
```

适合用来发现旧项目中被遗忘的 SSG，或者快速查看这台机器上哪些项目处于任意状态的 SSG。

## 互斥语义

每个会修改 SSG 的动词（`run`/`start`/`stop`/`restart`/`rm`/`checkout`/`uncheckout`）在分派到真实处理器之前，都会先在守护进程内部获取项目级 SSG 互斥锁。针对同一项目的两个并发调用会排队；针对不同项目则会并行运行。只读动词（`ps`/`ports`/`logs`/`exec`/`doctor`/`ls`）不会获取该互斥锁。

## Coastguard 集成

如果你正在运行 [Coastguard](../concepts_and_terminology/COASTGUARD.md)，SPA 会在它自己的页面（`/project/<p>/ssg/local`）上渲染 SSG 生命周期，并带有 Exec、Ports、Services、Logs、Secrets、Stats、Images 和 Volumes 标签页。每当某个消费者 Coast 触发自动启动时，都会触发 `CoastEvent::SsgStarting` 和 `CoastEvent::SsgStarted`，这样 UI 就可以将此次启动归因到需要它的那个项目。
