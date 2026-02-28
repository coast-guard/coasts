# Coast 守护进程

Coast 守护进程（`coastd`）是一个长期运行的本地进程，负责实际的编排工作。[CLI](CLI.md) 和 [Coastguard](COASTGUARD.md) 是客户端；`coastd` 是它们背后的控制平面。

## 架构概览

```text
coast CLI (automation) -----+
                            +--> coastd daemon
Coastguard UI (human) ------+         |
                                      +--> Coasts
                                      +--> Ports
                                      +--> State
```

CLI 通过本地 Unix 套接字向 `coastd` 发送请求；Coastguard 通过 WebSocket 连接。守护进程将变更应用到运行时状态。

## 它做什么

`coastd` 处理那些需要持久化状态和后台协调的操作:

- 跟踪 Coast 实例、构建以及共享服务。
- 创建、启动、停止并移除 Coast 运行时。
- 应用 assign/unassign/checkout 操作。
- 管理规范与动态的[端口转发](PORTS.md)。
- 向 CLI 和 UI 客户端流式传输[日志](LOGS.md)、状态以及运行时事件。

简而言之:如果你运行 `coast run`、`coast assign`、`coast checkout` 或 `coast ls`，做实际工作的组件就是该守护进程。

## 它如何运行

你可以通过两种常见方式运行守护进程:

```bash
# Register daemon auto-start at login (recommended)
coast daemon install

# Manual start mode
coast daemon start
```

如果你跳过守护进程安装，则每次会话在使用 Coast 命令之前都需要自己启动它。

## 报告 Bug

如果你遇到问题，在提交 bug 报告时请包含 `coastd` 守护进程日志。日志包含诊断大多数问题所需的上下文:

```bash
coast daemon logs
```
