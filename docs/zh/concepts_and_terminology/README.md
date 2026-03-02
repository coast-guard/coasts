# 概念与术语

本节涵盖贯穿 Coasts 的核心概念和词汇。如果你是 Coasts 新手，在深入配置或高级用法之前请先从这里开始。

- [Coasts](COASTS.md) — 你项目的自包含运行时，每个都有自己的端口、卷以及 worktree 分配。
- [Filesystem](FILESYSTEM.md) — 主机与 Coast 之间的共享挂载、主机侧代理，以及 worktree 切换。
- [Coast Daemon](DAEMON.md) — 本地的 `coastd` 控制平面，用于执行生命周期操作。
- [Coast CLI](CLI.md) — 用于命令、脚本和代理工作流的终端界面。
- [Coastguard](COASTGUARD.md) — 通过 `coast ui` 启动的 Web UI，用于可观测性与控制。
- [Ports](PORTS.md) — 规范端口与动态端口，以及 checkout 如何在它们之间切换。
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) — 指向主服务的快速链接、用于 Cookie 隔离的子域路由，以及 URL 模板。
- [Assign and Unassign](ASSIGN.md) — 在 worktree 之间切换一个 Coast 以及可用的 assign 策略。
- [Checkout](CHECKOUT.md) — 将规范端口映射到某个 Coast 实例，以及你何时需要它。
- [Lookup](LOOKUP.md) — 发现哪些 Coast 实例与代理当前的 worktree 匹配。
- [Volume Topology](VOLUMES.md) — 共享服务、共享卷、隔离卷与快照。
- [Shared Services](SHARED_SERVICES.md) — 主机管理的基础设施服务与卷消歧。
- [Secrets and Extractors](SECRETS.md) — 提取主机机密并将其注入到 Coast 容器中。
- [Builds](BUILDS.md) — coast build 的结构、产物存放位置、自动修剪与类型化构建。
- [Coastfile Types](COASTFILE_TYPES.md) — 可组合的 Coastfile 变体，支持 extends、unset、omit 与 autostart。
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) — DinD 运行时、Docker-in-Docker 架构，以及服务如何在 Coast 内运行。
- [Bare Services](BARE_SERVICES.md) — 在 Coast 内运行非容器化进程，以及为什么你应该改为容器化。
- [Logs](LOGS.md) — 从 Coast 内读取服务日志、MCP 的取舍，以及 Coastguard 日志查看器。
- [Exec & Docker](EXEC_AND_DOCKER.md) — 在 Coast 内运行命令并与内部 Docker 守护进程通信。
- [Agent Shells](AGENT_SHELLS.md) — 容器化的代理 TUI、OAuth 的取舍，以及为什么你可能应该改为在主机上运行代理。
- [MCP Servers](MCP_SERVERS.md) — 在 Coast 内为容器化代理配置 MCP 工具，内部服务器与主机代理服务器的区别。
- [Troubleshooting](TROUBLESHOOTING.md) — doctor、守护进程重启、项目移除，以及“出厂重置”核弹选项。
