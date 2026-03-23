# Exec 与 Docker

`coast exec` 会让你进入 Coast 的 DinD 容器内的一个 shell。你的工作目录是 `/workspace` —— 即 [bind-mounted project root](FILESYSTEM.md)，也就是你的 Coastfile 所在的位置。这是在宿主机上进入 Coast 内部运行命令、检查文件或调试服务的主要方式。

`coast docker` 是一个配套命令，用于直接与内部 Docker 守护进程通信。

## `coast exec`

在 Coast 实例内打开一个 shell:

```bash
coast exec dev-1
```

这会在 `/workspace` 启动一个 `sh` 会话。Coast 容器基于 Alpine，因此默认 shell 是 `sh`，而不是 `bash`。

你也可以不进入交互式 shell，直接运行指定命令:

```bash
coast exec dev-1 ls -la
coast exec dev-1 -- npm install
coast exec dev-1 -- go test ./...
coast exec dev-1 --service web
coast exec dev-1 --service web -- php artisan test
```

实例名后面的所有内容都会作为命令传递。使用 `--` 来分隔属于你命令的参数和属于 `coast exec` 的参数。

传入 `--service <name>` 可将目标指定为某个特定的 compose 服务容器，而不是外层的 Coast 容器。当你需要原始的容器 root 权限，而不是 Coast 默认的宿主机 UID:GID 映射时，传入 `--root`。

### 工作目录

shell 会从 `/workspace` 启动，它是绑定挂载到容器中的宿主机项目根目录。这意味着你的源代码、Coastfile 和所有项目文件都在那里:

```text
/workspace $ ls
Coastfile       README.md       apps/           packages/
Coastfile.light go.work         infra/          scripts/
Coastfile.snap  go.work.sum     package-lock.json
```

你对 `/workspace` 下文件所做的任何更改都会立即反映到宿主机上 —— 它是一个 bind mount，而不是副本。

### 交互式与非交互式

当 stdin 是 TTY（你正在终端中输入）时，`coast exec` 会完全绕过守护进程，直接运行 `docker exec -it` 以获得完整的 TTY 透传。这意味着颜色、光标移动、tab 补全以及交互式程序都能按预期工作。

当 stdin 通过管道传入或由脚本驱动时（CI、代理工作流、`coast exec dev-1 -- some-command | grep foo`），请求会通过守护进程处理，并返回结构化的 stdout、stderr 和退出码。

### 文件权限

exec 会以你的宿主机用户 UID:GID 运行，因此在 Coast 内创建的文件在宿主机上会拥有正确的所有权。宿主机与容器之间不会出现权限不匹配。

## `coast docker`

`coast exec` 会让你进入 DinD 容器本身的 shell，而 `coast docker` 则允许你针对**内部** Docker 守护进程运行 Docker CLI 命令 —— 也就是管理你的 compose 服务的那个守护进程。

```bash
coast docker dev-1                    # 默认等同于: docker ps
coast docker dev-1 ps                 # 与上面相同
coast docker dev-1 compose ps         # 对当前由 Coast 管理的栈执行 docker compose ps
coast docker dev-1 images             # 列出内部守护进程中的镜像
coast docker dev-1 compose logs web   # 某个服务的 docker compose logs
```

你传入的每条命令都会自动加上 `docker` 前缀。因此，`coast docker dev-1 compose ps` 会在 Coast 容器内运行 `docker compose ps`，并与内部守护进程通信。

### `coast exec` 与 `coast docker`

区别在于你要操作的目标是什么:

| Command | Runs as | Target |
|---|---|---|
| `coast exec dev-1 ls /workspace` | 在 DinD 容器中运行 `sh -c "ls /workspace"` | Coast 容器本身（你的项目文件、已安装工具） |
| `coast exec dev-1 --service web` | 在解析后的内部服务容器中运行 `docker exec ... sh` | 某个特定的 compose 服务容器 |
| `coast docker dev-1 ps` | 在 DinD 容器中运行 `docker ps` | 内部 Docker 守护进程（你的 compose 服务容器） |
| `coast docker dev-1 compose logs web` | 在 DinD 容器中运行 `docker compose logs web` | 通过内部守护进程查看特定 compose 服务的日志 |

项目级工作请使用 `coast exec` —— 例如运行测试、安装依赖、检查文件。当你需要查看内部 Docker 守护进程正在做什么时，请使用 `coast docker` —— 例如容器状态、镜像、网络、compose 操作。

## Coastguard Exec 标签页

Coastguard Web UI 提供了一个通过 WebSocket 连接的持久交互式终端。

![Exec tab in Coastguard](../../assets/coastguard-exec.png)
*Coastguard 的 Exec 标签页，显示了 Coast 实例内 `/workspace` 下的一个 shell 会话。*

该终端基于 xterm.js，并提供:

- **持久会话** —— 终端会话在页面跳转和浏览器刷新后依然保留。重新连接时会重放滚动缓冲区，因此你可以从上次离开的地方继续。
- **多个标签页** —— 可同时打开多个 shell。每个标签页都是独立会话。
- **[Agent shell](AGENT_SHELLS.md) 标签页** —— 为 AI 编码代理启动专用代理 shell，并跟踪其活动/非活动状态。
- **全屏模式** —— 将终端扩展为全屏显示（按 Escape 退出）。

除了实例级别的 exec 标签页外，Coastguard 还在其他层级提供终端访问:

- **服务 exec** —— 在 Services 标签页中点击某个单独服务，即可进入该特定内部容器中的 shell（这会执行双重 `docker exec` —— 先进入 DinD 容器，再进入服务容器）。
- **[共享服务](SHARED_SERVICES.md) exec** —— 进入宿主机级别共享服务容器中的 shell。
- **宿主机终端** —— 直接在项目根目录打开宿主机 shell，完全无需进入 Coast。

## 何时使用哪一个

- **`coast exec`** —— 在 DinD 容器内运行项目级命令，或传入 `--service` 以在特定 compose 服务容器内打开 shell 或运行命令。
- **`coast docker`** —— 检查或管理内部 Docker 守护进程（容器状态、镜像、网络、compose 操作）。
- **Coastguard Exec 标签页** —— 适用于带持久会话、多标签页和 agent shell 支持的交互式调试。当你希望在浏览 UI 其余部分时保持多个终端开启，这通常是最佳选择。
- **`coast logs`** —— 读取服务输出时，使用 `coast logs` 而不是 `coast docker compose logs`。参见 [Logs](LOGS.md)。
- **`coast ps`** —— 检查服务状态时，使用 `coast ps` 而不是 `coast docker compose ps`。参见 [Runtimes and Services](RUNTIMES_AND_SERVICES.md)。
