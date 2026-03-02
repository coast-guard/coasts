# Coasts 文档

## 安装

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*如果你决定不运行 `coast daemon install`，那么你需要负责每一次都手动使用 `coast daemon start` 启动守护进程。*

## 什么是 Coasts？

Coast（**容器化主机**）是一种本地开发运行时。Coasts 让你可以在一台机器上为同一个项目运行多个彼此隔离的环境。

Coasts 尤其适用于包含许多相互依赖服务的复杂 `docker-compose` 堆栈，但对于非容器化的本地开发设置也同样有效。Coasts 支持广泛的[运行时配置模式](concepts_and_terminology/RUNTIMES_AND_SERVICES.md)，因此你可以为多个并行工作的智能体打造理想环境。

Coasts 是为本地开发构建的，而不是作为托管云服务。你的环境会在你的本机上本地运行。

Coasts 项目是免费、本地、MIT 许可、与智能体提供方无关、与智能体编排工具无关的软件，并且没有任何 AI 增值推销。

Coasts 可与任何使用 worktrees 的智能体编程工作流配合使用。不需要任何编排端的特殊配置。

## 为什么 Coasts 适用于 Worktrees

Git worktrees 非常适合隔离代码变更，但它们本身并不能解决运行时隔离问题。

当你并行运行多个 worktree 时，很快就会遇到易用性问题:

- 在期望使用相同宿主机端口的服务之间产生的[端口冲突](concepts_and_terminology/PORTS.md)。
- 每个 worktree 的数据库与[卷设置](concepts_and_terminology/VOLUMES.md)管理起来很繁琐。
- 需要为每个 worktree 进行自定义运行时布线的集成测试环境。
- 每次切换 worktree 并重新构建运行时上下文的活地狱。参见[Assign and Unassign](concepts_and_terminology/ASSIGN.md)。

如果说 Git 是你代码的版本控制，那么 Coasts 就像是你的 worktree 运行时的 Git。

每个环境都有自己的端口，因此你可以并行检查任何 worktree 运行时。当你[check out](concepts_and_terminology/CHECKOUT.md) 一个 worktree 运行时时，Coasts 会将该运行时重映射到你项目的规范端口。

Coasts 将运行时配置抽象为 worktrees 之上的一个简单模块化层，因此每个 worktree 都能在所需的隔离下运行，而无需手动维护复杂的按 worktree 划分的设置。

## 要求

- macOS
- Docker Desktop
- 使用 Git 的项目
- Node.js
- `socat` *(通过 `curl -fsSL https://coasts.dev/install | sh` 安装时作为 Homebrew `depends_on` 依赖一并安装)*

```text
Linux 注意:我们尚未在 Linux 上测试 Coasts，但计划支持 Linux。
你今天可以尝试在 Linux 上运行 Coasts，但我们不保证它会正常工作。
```

## 将智能体容器化？

你可以用 Coast 将智能体容器化。乍听起来这可能是个好主意，但在许多情况下，你实际上并不需要在容器里运行你的编程智能体。

因为 Coasts 通过共享卷挂载与宿主机共享[文件系统](concepts_and_terminology/FILESYSTEM.md)，最简单且最可靠的工作流是在宿主机上运行智能体，并指示它使用 [`coast exec`](concepts_and_terminology/EXEC_AND_DOCKER.md) 在 Coast 实例内执行运行时开销较大的任务（例如集成测试）。

不过，如果你确实想在容器中运行智能体，Coasts 也完全支持:通过[Agent Shells](concepts_and_terminology/AGENT_SHELLS.md)。你可以为此搭建一个极其复杂的装备，包括 [MCP 服务器配置](concepts_and_terminology/MCP_SERVERS.md)，但它可能无法与当今已有的编排软件很好地互操作。对大多数工作流而言，宿主机侧智能体更简单且更可靠。

## Coasts vs Dev Containers

Coasts 不是 dev containers，它们也不是同一种东西。

Dev containers 通常用于将 IDE 挂载到单个容器化的开发工作区中。Coasts 是无头（headless）的，并针对与 worktrees 的并行智能体使用优化为轻量级环境——多个彼此隔离、具备 worktree 感知的运行时环境并排运行，具备快速的 checkout 切换，以及对每个实例的运行时隔离控制。
