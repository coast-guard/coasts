# 故障排除

Coasts 的大多数问题源于陈旧的状态、孤立的 Docker 资源，或守护进程不同步。本页涵盖从温和到核选项的升级处理路径。

## Doctor

如果感觉不对劲——实例显示为运行中但没有任何响应、端口似乎卡住、或 UI 显示过期数据——先从 `coast doctor` 开始:

```bash
coast doctor
```

Doctor 会扫描状态数据库和 Docker 以查找不一致:容器缺失的孤立实例记录、没有状态记录的悬挂容器，以及被标记为运行但实际上已挂掉的共享服务。它会自动修复发现的问题。

如需预览将执行的操作而不做任何更改:

```bash
coast doctor --dry-run
```

## Daemon Restart

如果守护进程本身似乎无响应，或你怀疑它处于不良状态，请重启它:

```bash
coast daemon restart
```

这会发送优雅关闭信号，等待守护进程退出，并启动一个全新的进程。你的实例和状态会被保留。

## Removing a Single Project

如果问题仅限于某一个项目，你可以删除其构建产物及关联的 Docker 资源，而不影响其他任何内容:

```bash
coast rm-build my-project
```

这会删除该项目的产物目录、Docker 镜像、卷以及容器。它会先请求确认。传入 `--force` 可跳过提示。

## Missing Shared Service Images

如果 `coast run` 在创建共享服务时失败，并出现类似 `No such image: postgres:15` 的错误，那么该镜像在你的宿主机 Docker 守护进程中缺失。

这最常发生在你的 `Coastfile` 定义了诸如 Postgres 或 Redis 的 `shared_services`，但 Docker 尚未拉取这些镜像时。

拉取缺失的镜像，然后再次运行该实例:

```bash
docker pull postgres:15
docker pull redis:7
coast run my-instance
```

如果你不确定缺的是哪个镜像，失败的 `coast run` 输出会在 Docker 错误中包含镜像名称。一次失败的置备尝试之后，Coasts 会自动清理部分创建的实例，因此看到实例回到 `stopped` 属于预期行为。

## Factory Reset with Nuke

当其他方法都无效——或者你只是想要一个完全干净的环境——`coast nuke` 会执行完整的出厂重置:

```bash
coast nuke
```

这将会:

1. 停止 `coastd` 守护进程。
2. 移除 **所有** 由 coast 管理的 Docker 容器。
3. 移除 **所有** 由 coast 管理的 Docker 卷。
4. 移除 **所有** 由 coast 管理的 Docker 网络。
5. 移除 **所有** coast Docker 镜像。
6. 删除整个 `~/.coast/` 目录（状态数据库、构建、日志、密钥、镜像缓存）。
7. 重新创建 `~/.coast/` 并重启守护进程，使 coast 能立即再次使用。

由于这会销毁一切，你必须在确认提示中输入 `nuke`:

```text
$ coast nuke
WARNING: This will permanently destroy ALL coast data:

  - Stop the coastd daemon
  - Remove all coast-managed Docker containers
  - Remove all coast-managed Docker volumes
  - Remove all coast-managed Docker networks
  - Remove all coast Docker images
  - Delete ~/.coast/ (state DB, builds, logs, secrets, image cache)

Type "nuke" to confirm:
```

传入 `--force` 可跳过提示（在脚本中很有用）:

```bash
coast nuke --force
```

nuke 之后，coast 已准备好使用——守护进程在运行，且主目录已存在。你只需要再次对项目执行 `coast build` 和 `coast run`。

## Reporting Bugs

如果你遇到的问题无法通过以上任何方法解决，请在报告时附上守护进程日志:

```bash
coast daemon logs
```
