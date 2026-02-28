# Exec & Docker

`coast exec` 会将你带入 Coast 的 DinD 容器内的一个 shell。你的工作目录是 `/workspace` —— [绑定挂载的项目根目录](FILESYSTEM.md)，也就是你的 Coastfile 所在的位置。这是在主机上运行命令、检查文件或调试 Coast 内服务的主要方式。

`coast docker` 是用于直接与内部 Docker 守护进程通信的配套命令。

## `coast exec`

在某个 Coast 实例内打开一个 shell:

```bash
coast exec dev-1
```

这会在 `/workspace` 启动一个 `sh` 会话。Coast 容器基于 Alpine，因此默认 shell 是 `sh`，而不是 `bash`。

你也可以在不进入交互式 shell 的情况下运行特定命令:

```bash
coast exec dev-1 ls -la
coast exec dev-1 -- npm install
coast exec dev-1 -- go test ./...
```

实例名称之后的所有内容都会作为命令传递。使用 `--` 将属于你的命令的标志与属于 `coast exec` 的标志分开。

### Working Directory

shell 从 `/workspace` 启动，它是你主机项目根目录绑定挂载到容器中的位置。这意味着你的源代码、Coastfile 以及所有项目文件都在那里:

```text
/workspace $ ls
Coastfile       README.md       apps/           packages/
Coastfile.light go.work         infra/          scripts/
Coastfile.snap  go.work.sum     package-lock.json
```

你在 `/workspace` 下对文件所做的任何更改都会立即反映到主机上——这是绑定挂载，而不是拷贝。

### Interactive vs Non-Interactive

当 stdin 是 TTY（你在终端里输入）时，`coast exec` 会完全绕过守护进程，直接运行 `docker exec -it` 以获得完整的 TTY 透传。这意味着颜色、光标移动、Tab 补全以及交互式程序都能按预期工作。

当 stdin 是通过管道传入或脚本化（CI、agent 工作流、`coast exec dev-1 -- some-command | grep foo`）时，请求会经过守护进程，并返回结构化的 stdout、stderr 和退出码。

### File Permissions

exec 以你主机用户的 UID:GID 运行，因此在 Coast 内创建的文件在主机上拥有正确的所有权。主机与容器之间不会出现权限不匹配。

## `coast docker`

`coast exec` 让你进入 DinD 容器本身的 shell，而 `coast docker` 让你针对 **内部** Docker 守护进程运行 Docker CLI 命令——也就是管理你的 compose 服务的那个守护进程。

```bash
coast docker dev-1                    # defaults to: docker ps
coast docker dev-1 ps                 # same as above
coast docker dev-1 compose ps         # docker compose ps (inner services)
coast docker dev-1 images             # list images in the inner daemon
coast docker dev-1 compose logs web   # docker compose logs for a service
```

你传入的每条命令都会自动加上 `docker` 前缀。因此 `coast docker dev-1 compose ps` 会在 Coast 容器内运行 `docker compose ps`，并与内部守护进程通信。

### `coast exec` vs `coast docker`

区别在于你要操作的目标:

| Command | Runs as | Target |
|---|---|---|
| `coast exec dev-1 ls /workspace` | `sh -c "ls /workspace"` in DinD container | Coast 容器本身（你的项目文件、已安装的工具） |
| `coast docker dev-1 ps` | `docker ps` in DinD container | 内部 Docker 守护进程（你的 compose 服务容器） |
| `coast docker dev-1 compose logs web` | `docker compose logs web` in DinD container | 通过内部守护进程获取特定 compose 服务的日志 |

将 `coast exec` 用于项目级工作——运行测试、安装依赖、检查文件。需要查看内部 Docker 守护进程在做什么时——容器状态、镜像、网络、compose 操作——使用 `coast docker`。

## Coastguard Exec Tab

Coastguard Web UI 提供了一个通过 WebSocket 连接的持久交互式终端。

![Exec tab in Coastguard](../../assets/coastguard-exec.png)
*Coastguard 的 Exec 标签页，展示了在某个 Coast 实例内 /workspace 目录的 shell 会话。*

该终端由 xterm.js 驱动，并提供:

- **持久会话** — 终端会话在页面导航和浏览器刷新后仍然保留。重新连接会回放滚动缓冲区，因此你可以从离开的地方继续。
- **多个标签页** — 同时打开多个 shell。每个标签页都是独立会话。
- **[Agent shell](AGENT_SHELLS.md) 标签页** — 为 AI 编码 agent 生成专用的 agent shell，并跟踪活跃/非活跃状态。
- **全屏模式** — 将终端扩展为全屏（按 Escape 退出）。

除了实例级的 exec 标签页之外，Coastguard 还在其他层级提供终端访问:

- **服务 exec** — 从 Services 标签页进入某个单独服务，以在该特定内部容器中获得一个 shell（这会执行双重 `docker exec`——先进入 DinD 容器，再进入服务容器）。
- **[共享服务](SHARED_SERVICES.md) exec** — 在主机级共享服务容器内获得一个 shell。
- **主机终端** — 在你的主机机器上、项目根目录处的 shell，无需进入任何 Coast。

## When to Use Which

- **`coast exec`** — 在 DinD 容器内运行项目级命令（npm install、go test、文件检查、调试）。
- **`coast docker`** — 检查或管理内部 Docker 守护进程（容器状态、镜像、网络、compose 操作）。
- **Coastguard Exec tab** — 通过持久会话、多标签页和 agent shell 支持进行交互式调试。当你希望在浏览 UI 其他部分时保持多个终端打开，这是最佳选择。
- **`coast logs`** — 读取服务输出时，使用 `coast logs` 而不是 `coast docker compose logs`。参见 [Logs](LOGS.md)。
- **`coast ps`** — 检查服务状态时，使用 `coast ps` 而不是 `coast docker compose ps`。参见 [Runtimes and Services](RUNTIMES_AND_SERVICES.md)。
