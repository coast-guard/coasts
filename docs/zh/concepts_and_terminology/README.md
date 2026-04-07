# 概念与术语

本节介绍贯穿 Coasts 的核心概念和术语。如果你刚接触 Coasts，请先从这里开始，再深入了解配置或高级用法。

- [Coasts](COASTS.md) - 你项目的自包含运行时，每个都有自己的端口、卷和工作树分配。
- [Run](RUN.md) - 从最新构建创建一个新的 Coast 实例，并可选择分配一个工作树。
- [Remove](REMOVE.md) - 在你需要干净地重新创建或想要关闭 Coasts 时，拆除一个 Coast 实例及其隔离的运行时状态。
- [Filesystem](FILESYSTEM.md) - 主机与 Coast 之间的共享挂载、主机侧代理以及工作树切换。
- [Private Paths](PRIVATE_PATHS.md) - 针对在共享绑定挂载中发生冲突的工作区路径提供按实例隔离。
- [Coast Daemon](DAEMON.md) - 执行生命周期操作的本地 `coastd` 控制平面。
- [Coast CLI](CLI.md) - 用于命令、脚本和代理工作流的终端界面。
- [Coastguard](COASTGUARD.md) - 使用 `coast ui` 启动的 Web UI，用于可观测性和控制。
- [Ports](PORTS.md) - 规范端口与动态端口，以及 checkout 如何在它们之间切换。
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - 指向你的主服务的快速链接、用于 Cookie 隔离的子域路由，以及 URL 模板。
- [Assign and Unassign](ASSIGN.md) - 在工作树之间切换 Coast，以及可用的分配策略。
- [Checkout](CHECKOUT.md) - 将规范端口映射到某个 Coast 实例，以及你何时需要它。
- [Lookup](LOOKUP.md) - 发现哪些 Coast 实例与代理当前的工作树匹配。
- [Volume Topology](VOLUMES.md) - 共享服务、共享卷、隔离卷以及快照。
- [Shared Services](SHARED_SERVICES.md) - 主机管理的基础设施服务和卷消歧。
- [Secrets and Extractors](SECRETS.md) - 提取主机密钥并将其注入到 Coast 容器中。
- [Builds](BUILDS.md) - coast 构建的组成、产物存放位置、自动清理以及类型化构建。
- [Coastfile Types](COASTFILE_TYPES.md) - 具有 extends、unset、omit 和 autostart 的可组合 Coastfile 变体。
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) - DinD 运行时、Docker-in-Docker 架构，以及服务如何在 Coast 内运行。
- [Bare Services](BARE_SERVICES.md) - 在 Coast 内运行非容器化进程，以及为什么你应该改为使用容器化。
- [Bare Service Optimization](BARE_SERVICE_OPTIMIZATION.md) - 针对裸服务的条件安装、缓存、private_paths、共享服务连接和分配策略。
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - 自动注入的 `<SERVICE>_DYNAMIC_PORT` 变量，以及如何在服务命令中使用它们。
- [Logs](LOGS.md) - 从 Coast 内部读取服务日志、MCP 权衡，以及 Coastguard 日志查看器。
- [Exec & Docker](EXEC_AND_DOCKER.md) - 在 Coast 内运行命令并与内部 Docker 守护进程通信。
- [Agent Shells](AGENT_SHELLS.md) - 容器化代理 TUI、OAuth 权衡，以及为什么你或许更应该在主机上运行代理。
- [MCP Servers](MCP_SERVERS.md) - 为容器化代理在 Coast 内配置 MCP 工具、内部服务器与主机代理服务器。
- [Remotes](REMOTES.md) - 通过 coast-service 在远程机器上运行服务，同时保持本地工作流不变。
- [Troubleshooting](TROUBLESHOOTING.md) - doctor、守护进程重启、项目移除，以及恢复出厂设置式的清空选项。
