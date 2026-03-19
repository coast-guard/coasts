# Coasts 文档

```youtube
MBGKSKau4sU
Part of the [Coasts Video Course](learn-coasts-videos/README.md).
```

## 安装

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*如果你决定不运行 `coast daemon install`，那么你需要负责在每一次都手动通过 `coast daemon start` 启动守护进程。*

## 什么是 Coasts？

一个 Coast（**容器化主机**）是本地开发运行时。Coasts 让你可以在同一台机器上为同一个项目运行多个彼此隔离的环境。

Coasts 对于包含许多相互依赖服务的复杂 `docker-compose` 栈尤其有用，但它们对非容器化的本地开发设置同样有效。Coasts 支持广泛的[运行时配置模式](concepts_and_terminology/RUNTIMES_AND_SERVICES.md)，因此你可以为多个并行工作的代理塑造理想环境。

Coasts 是为本地开发而构建的，而不是作为托管的云服务。你的环境会在你的机器上本地运行。

Coasts 项目是免费、本地、MIT 许可、与代理提供方无关、与代理编排工具无关的软件，没有任何 AI 加购。

Coasts 可与任何使用 worktrees 的代理式编码工作流配合使用。不需要在编排工具侧做任何特殊配置。

## 为什么 Coasts 适用于 Worktrees

Git worktrees 非常适合隔离代码变更，但它们本身并不能解决运行时隔离。

当你并行运行多个 worktrees 时，很快就会遇到易用性问题:

- 在期望使用相同主机端口的服务之间发生[端口冲突](concepts_and_terminology/PORTS.md)。
- 每个 worktree 的数据库与[卷设置](concepts_and_terminology/VOLUMES.md)管理起来很繁琐。
- 需要为每个 worktree 定制运行时接线的集成测试环境。
- 在不同 worktree 之间切换并每次都重建运行时上下文的“人间地狱”。参见 [Assign and Unassign](concepts_and_terminology/ASSIGN.md)。

如果说 Git 是你的代码的版本控制，那么 Coasts 就像是你的 worktree 运行时的 Git。

每个环境都会获得自己的一组端口，因此你可以并行检查任何 worktree 运行时。当你[检出](concepts_and_terminology/CHECKOUT.md)某个 worktree 运行时时，Coasts 会将该运行时重新映射到你项目的规范端口。

Coasts 将运行时配置抽象为位于 worktrees 之上的一个简单模块化层，这样每个 worktree 都能以所需的隔离方式运行，而无需手动维护复杂的按 worktree 划分的配置。

## 要求

- macOS 或 Linux
- macOS 上使用 Docker Desktop，或 Linux 上使用带 Compose 插件的 Docker Engine
- 一个使用 Git 的项目
- Node.js
- `socat`（macOS 上使用 `brew install socat`，Ubuntu 上使用 `sudo apt install socat`）

```text
Linux 注意:动态端口在 Linux 上开箱即用。
如果你需要低于 `1024` 的规范端口，请参阅 checkout 文档了解所需的主机配置。
```

## 将代理容器化？

你可以用一个 Coast 将代理容器化。乍一听这似乎是个好主意，但在很多情况下，你其实并不需要把你的编码代理运行在容器里。

因为 Coasts 通过共享卷挂载与宿主机共享[文件系统](concepts_and_terminology/FILESYSTEM.md)，最简单且最可靠的工作流是:在宿主机上运行代理，并指示它使用 [`coast exec`](concepts_and_terminology/EXEC_AND_DOCKER.md) 在 Coast 实例内执行运行时开销较大的任务（例如集成测试）。

不过，如果你确实想在容器中运行你的代理，Coasts 当然也支持通过 [Agent Shells](concepts_and_terminology/AGENT_SHELLS.md) 来实现。你可以为该设置构建一个极其复杂的装置，包括 [MCP 服务器配置](concepts_and_terminology/MCP_SERVERS.md)，但它可能无法与当今已有的编排软件良好互操作。对大多数工作流而言，宿主机侧代理更简单也更可靠。

## Coasts vs Dev Containers

Coasts 不是 dev containers，它们也不是同一种东西。

Dev containers 通常旨在将一个 IDE 挂载到单个容器化的开发工作区中。Coasts 则是无头的，并且针对使用 worktrees 的并行代理使用场景进行了优化——多个彼此隔离、具备 worktree 感知的运行时环境并排运行，具备快速的检出切换以及针对每个实例的运行时隔离控制。

## 演示仓库

如果你想要一个小型示例项目来试用 Coasts，可以从 [`coasts-demo` 仓库](https://github.com/coast-guard/coasts-demo)开始。

## Coasts 视频课程

如果你更喜欢视频，[Coasts Video Course](learn-coasts-videos/README.md) 会在每节不超过三分钟的内容中讲解每一个核心概念。
