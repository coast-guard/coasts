# `coast ssg` CLI 参考

每个 `coast ssg` 子命令都会通过现有的 Unix 套接字与同一个本地守护进程通信。`coast shared-service-group` 是 `coast ssg` 的别名。

大多数动词都会从当前工作目录的 `Coastfile` 中的 `[coast].name`（或 `--working-dir <dir>`）解析项目。只有 `coast ssg ls` 是跨项目的。

所有命令都接受全局 `--silent` / `-s` 标志，用于抑制进度输出，只打印最终摘要或错误。

## Commands

### Build & inspect

| Command | Summary |
|---------|---------|
| `coast ssg build [-f <file>] [--working-dir <dir>] [--config '<toml>']` | 解析 `Coastfile.shared_service_groups`，提取所有 `[secrets.*]`，拉取镜像，将构件写入 `~/.coast/ssg/<project>/builds/<id>/`，更新 `latest_build_id`，清理旧构建。参见 [Building](BUILDING.md)。 |
| `coast ssg ps` | 显示此项目的 SSG 构建服务列表（读取 `manifest.json` 以及实时容器状态）。参见 [Lifecycle -> ps](LIFECYCLE.md#coast-ssg-ps)。 |
| `coast ssg builds-ls [--working-dir <dir>] [-f <file>]` | 列出 `~/.coast/ssg/<project>/builds/` 下的每个构建构件，包含时间戳、服务数量以及 `(latest)` / `(pinned)` 标注。 |
| `coast ssg ls` | 跨项目列出守护进程已知的每个 SSG（项目、状态、构建 id、服务数量、创建时间）。参见 [Lifecycle -> ls](LIFECYCLE.md#coast-ssg-ls)。 |

### Lifecycle

| Command | Summary |
|---------|---------|
| `coast ssg run` | 创建 `<project>-ssg` DinD，分配动态主机端口，实体化 secrets（声明时），启动内部 compose 栈。参见 [Lifecycle -> run](LIFECYCLE.md#coast-ssg-run)。 |
| `coast ssg start` | 启动先前已创建但已停止的 SSG。重新实体化 secrets，并重新生成所有保留的规范端口 checkout socat。 |
| `coast ssg stop [--force]` | 停止该项目的 SSG DinD。保留容器、动态端口、虚拟端口和 checkout 记录。`--force` 会先拆除远程 SSH 隧道。 |
| `coast ssg restart` | 停止 + 启动。保留容器和动态端口。 |
| `coast ssg rm [--with-data] [--force]` | 删除该项目的 SSG DinD。`--with-data` 会删除内部命名卷。`--force` 会在存在远程 shadow 使用者时继续执行。主机 bind-mount 内容永远不会被触碰。**Keystore 永远不会被触碰** —— 如需清除请使用 `coast ssg secrets clear`。 |

### Logs & exec

| Command | Summary |
|---------|---------|
| `coast ssg logs [--service <name>] [--tail N] [--follow]` | 流式输出外层 DinD 或某个内部服务的日志。`--follow` 会持续流式输出直到 Ctrl+C。 |
| `coast ssg exec [--service <name>] -- <cmd...>` | 进入外层 `<project>-ssg` 容器或某个内部服务执行命令。`--` 之后的所有内容都会原样透传。 |

### Routing & checkout

| Command | Summary |
|---------|---------|
| `coast ssg ports` | 显示每个服务的规范 / 动态 / 虚拟端口映射，并在适用时附加 `(checked out)` 标注。参见 [Routing](ROUTING.md)。 |
| `coast ssg checkout [--service <name> \| --all]` | 通过主机侧 socat 绑定规范主机端口（转发器目标为该项目的稳定虚拟端口）。会以警告方式替换由 Coast 实例持有的端口；若是未知主机进程则报错。参见 [Checkout](CHECKOUT.md)。 |
| `coast ssg uncheckout [--service <name> \| --all]` | 为该项目拆除规范端口 socat。不会自动恢复被替换的 Coast。 |

### Diagnostics

| Command | Summary |
|---------|---------|
| `coast ssg doctor` | 只读检查:针对已知镜像服务的主机 bind-mount 权限，以及已声明但未提取的 SSG secrets。输出 `ok` / `warn` / `info` 结果。参见 [Volumes -> coast ssg doctor](VOLUMES.md#coast-ssg-doctor)。 |

### Build pinning

| Command | Summary |
|---------|---------|
| `coast ssg checkout-build <BUILD_ID> [--working-dir <dir>] [-f <file>]` | 将此项目的 SSG 固定到特定的 `build_id`。`coast ssg run` 和 `coast build` 将使用该固定值而不是 `latest_build_id`。参见 [Building -> Locking a project to a specific build](BUILDING.md#locking-a-project-to-a-specific-build)。 |
| `coast ssg uncheckout-build [--working-dir <dir>] [-f <file>]` | 释放该固定。幂等。 |
| `coast ssg show-pin [--working-dir <dir>] [-f <file>]` | 显示此项目当前的固定值（如果有）。 |

### SSG-native secrets

| Command | Summary |
|---------|---------|
| `coast ssg secrets clear` | 删除 `coast_image = "ssg:<project>"` 下的所有加密 keystore 条目。幂等。这是唯一会清除 SSG-native secrets 的动词 —— `coast ssg rm` 和 `rm --with-data` 会刻意保留它们。参见 [Secrets](SECRETS.md)。 |

### Migration helper

| Command | Summary |
|---------|---------|
| `coast ssg import-host-volume <VOLUME> --service <name> --mount <path> [--apply] [-f <file>] [--working-dir <dir>] [--config '<toml>']` | 解析主机 Docker 命名卷的挂载点，并输出（或应用）等效的 SSG bind-mount 条目。参见 [Volumes -> coast ssg import-host-volume](VOLUMES.md#automating-the-recipe-coast-ssg-import-host-volume)。 |

## Exit Codes

- `0` -- 成功。像 `doctor` 这样的命令即使发现警告也会返回 0；它们是诊断工具，而不是门禁。
- 非零 -- 校验错误、Docker 错误、状态不一致，或远程 shadow gate 拒绝。

## See Also

- [Building](BUILDING.md)
- [Lifecycle](LIFECYCLE.md)
- [Routing](ROUTING.md)
- [Volumes](VOLUMES.md)
- [Consuming](CONSUMING.md)
- [Secrets](SECRETS.md)
- [Checkout](CHECKOUT.md)
