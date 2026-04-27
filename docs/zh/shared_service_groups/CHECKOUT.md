# SSG 主机侧 Checkout

Consumer Coast 通过守护进程的路由层访问 SSG 服务（in-DinD socat -> 主机 socat -> 动态端口）。这对于应用容器来说很好用。但它无法帮助主机侧调用方——MCP、临时的 `psql` 会话、你编辑器的数据库检查器——这些调用方希望连接到 `localhost:5432`，就像服务就运行在那里一样。

`coast ssg checkout` 就是为此而生。它会生成一个主机级别的 socat，绑定规范的主机端口（Postgres 为 5432，Redis 为 6379，……），并转发到项目的稳定虚拟端口。随后，主机现有的虚拟端口 socat 会将流量继续转发到 SSG 当前发布的动态端口。

整个机制是按项目划分的。`coast ssg checkout --service postgres` 会解析到拥有当前工作目录 `Coastfile` 的项目；如果这台机器上有两个项目，同一时间只能有一个占用规范端口 5432。

## 用法

```bash
coast ssg checkout --service postgres     # bind one service
coast ssg checkout --all                  # bind every SSG service
coast ssg uncheckout --service postgres   # tear down one
coast ssg uncheckout --all                # tear down every active checkout
```

成功 checkout 之后，`coast ssg ports` 会用 `(checked out)` 标注每个已绑定的服务:

```text
  SERVICE              CANONICAL       DYNAMIC         VIRTUAL    STATUS
  postgres             5432            54201           42000      (checked out)
  redis                6379            54202           42001
```

无论主机侧 checkout 状态如何，Consumer Coast 始终通过其 in-DinD socat -> 虚拟端口链路访问 SSG 服务。Checkout 纯粹是主机侧的便利功能。

## 双跳转发器

checkout socat **不会** 直接指向 SSG 的动态主机端口。它指向的是项目的稳定虚拟端口:

```text
host process            -> 127.0.0.1:5432           (checkout socat, listens here)
                        -> 127.0.0.1:42000          (project's virtual port)
                        -> 127.0.0.1:54201          (SSG's current dynamic port)
                        -> <project>-ssg postgres   (inner service)
```

这种双跳链路意味着，即使动态端口发生变化，checkout socat 在 SSG 重建后仍然可以继续工作。只有主机的虚拟端口 socat 需要更新——规范端口 socat 并不知情。关于主机 socat 层如何维护，请参见 [Routing](ROUTING.md)。

## 对 Coast 实例持有者的置换

当你要求 SSG checkout 一个规范端口时，该端口可能已经被占用。语义取决于占用者是谁:

- **一个被显式 checkout 的 Coast 实例。** 今天早些时候，某个 Coast 上的 `coast checkout <instance>` 已将 `localhost:5432` 绑定到该 Coast 的内部 Postgres。SSG checkout 会**置换**它:守护进程会杀掉现有 socat，清除该 Coast 的 `port_allocations.socat_pid`，然后改为绑定 SSG 的 socat。CLI 会打印清晰的警告:

  ```text
  Warning: displaced coast 'my-app/dev-2' from canonical port 5432.
  SSG checkout complete: postgres on canonical 5432 -> virtual 42000.
  ```

  被置换的 Coast **不会** 在你稍后执行 `coast ssg uncheckout` 时自动重新绑定。它的动态端口仍然可用，但规范端口会保持未绑定状态，直到你再次运行 `coast checkout my-app/dev-2`。

- **另一个项目的 SSG checkout。** 如果 `filemap-ssg` 已经 checkout 了 5432，而你尝试 checkout `cg-ssg` 的 5432，守护进程会拒绝，并给出明确消息指出占用者。请先 uncheckout `filemap-ssg` 的 5432。

- **一条之前的 SSG checkout 记录，其 `socat_pid` 已失效。** 这是来自守护进程崩溃或 stop/start 周期的陈旧元数据。新的 checkout 会静默回收这条记录。

- **其他任何情况**（你手动启动的主机 Postgres、另一个守护进程、运行在 8080 端口上的 `nginx`）。`coast ssg checkout` 会报错:

  ```text
  error: port 5432 is held by a process outside Coast's tracking. Free the port with `lsof -i :5432` / `kill <pid>` and retry.
  ```

  没有 `--force` 标志。静默杀掉未知进程被认为风险过高。

## Stop / Start 行为

`coast ssg stop` 会杀掉正在运行的规范端口 socat 进程，但**会保留 checkout 记录本身**在状态 DB 中。

`coast ssg run` / `start` / `restart` 会遍历这些保留的记录，并为每条记录重新生成一个新的规范端口 socat。规范端口（5432）保持不变；只有动态端口会在不同的 `run` 周期之间变化，而由于 checkout socat 目标是**虚拟**端口（它同样是稳定的），因此重新绑定只是机械性的操作。

如果某个服务从重建后的 SSG 中消失，它的 checkout 记录会被删除，并在 run 响应中给出警告:

```text
SSG run complete. Dropped stale checkout for service 'mongo' (no longer in the active SSG build).
```

`coast ssg rm` 会清除该项目的所有 `ssg_port_checkouts` 记录。`rm` 按设计就是破坏性的——你明确要求了一个干净的初始状态。

## 守护进程重启恢复

在守护进程意外重启之后（崩溃、`coastd restart`、重启机器），`restore_running_state` 会查询 `ssg_port_checkouts` 表，并根据当前的动态 / 虚拟端口分配重新生成每一条记录。你的 `localhost:5432` 会在守护进程波动后继续保持绑定。

## 何时应该 Check Out

- 你想让某个 GUI 数据库客户端指向项目的 SSG Postgres。
- 你希望 `psql "postgres://coast:coast@localhost:5432/mydb"` 可以直接工作，而不必先找出动态端口。
- 你主机上的某个 MCP 需要一个稳定的规范端点。
- Coastguard 希望代理 SSG 的 HTTP 管理端口。

何时**不**应 checkout:

- 用于从 consumer Coast 内部进行连通性访问——那已经可以通过 in-DinD socat 到虚拟端口实现。
- 当你满足于使用 `coast ssg ports` 的输出，并将动态端口填入你的工具时。

## 另请参见

- [Routing](ROUTING.md) -- 规范 / 动态 / 虚拟端口概念，以及完整的主机侧转发器链路
- [Lifecycle](LIFECYCLE.md) -- stop / start / rm 细节
- [Coast Checkout](../concepts_and_terminology/CHECKOUT.md) -- 这一思想的 Coast 实例版本
- [Ports](../concepts_and_terminology/PORTS.md) -- 整个系统中规范端口与动态端口的管线机制
