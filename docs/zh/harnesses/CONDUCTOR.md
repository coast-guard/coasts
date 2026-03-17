# Conductor

[Conductor](https://conductor.build/) 运行并行的 Claude Code 代理，每个代理都在其各自隔离的工作区中。工作区是存储在 `~/conductor/workspaces/<project-name>/` 的 git worktree。每个工作区都会检出到一个命名分支上。

由于这些 worktree 位于项目根目录之外，Coast 需要显式配置才能发现并挂载它们。

## 设置

将 `~/conductor/workspaces/<project-name>` 添加到 `worktree_dir`。与 Codex（它将所有项目存储在一个扁平目录下）不同，Conductor 将 worktree 嵌套在按项目划分的子目录中，因此路径必须包含项目名称。在下面的示例中，`my-app` 必须与你的仓库在 `~/conductor/workspaces/` 下的实际文件夹名称一致。

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/conductor/workspaces/my-app"]
```

Conductor 允许你为每个仓库配置工作区路径，因此默认的 `~/conductor/workspaces` 可能与你的设置不一致。请检查你的 Conductor 仓库设置以找到实际路径，并相应调整——无论目录位于何处，原理都是一样的。

Coast 在运行时展开 `~`，并将任何以 `~/` 或 `/` 开头的路径视为外部路径。详见 [Worktree Directories](../coastfiles/WORKTREE_DIR.md)。

更改 `worktree_dir` 后，必须**重新创建**现有实例，绑定挂载才能生效:

```bash
coast rm my-instance
coast build
coast run my-instance
```

worktree 列表会立即更新（Coast 会读取新的 Coastfile），但分配到 Conductor worktree 需要容器内存在绑定挂载。

## Coast 会做什么

- **绑定挂载** — 在容器创建时，Coast 会将 `~/conductor/workspaces/<project-name>` 挂载到容器中的 `/host-external-wt/{index}`。
- **发现** — `git worktree list --porcelain` 的作用域是仓库级别，因此只会显示属于当前项目的 worktree。
- **命名** — Conductor worktree 使用命名分支，因此它们会在 Coast UI 和 CLI 中以分支名显示（例如，`scroll-to-bottom-btn`）。一个分支在同一时间只能在一个 Conductor 工作区中被检出。
- **分配** — `coast assign` 会从外部绑定挂载路径重新挂载 `/workspace`。
- **Gitignored 同步** — 在主机文件系统上使用绝对路径运行，无需绑定挂载即可工作。
- **孤儿检测** — git 监视器会递归扫描外部目录，并通过 `.git` gitdir 指针进行过滤。如果 Conductor 归档或删除某个工作区，Coast 会自动取消该实例的分配。

## 示例

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/conductor/workspaces/my-app"]
primary_port = "web"

[ports]
web = 3000
api = 8080

[assign]
default = "none"
[assign.services]
web = "hot"
api = "hot"
```

- `.worktrees/` — Coast 管理的 worktree
- `.claude/worktrees/` — Claude Code（本地，无需特殊处理）
- `~/.codex/worktrees/` — Codex（外部，绑定挂载）
- `~/conductor/workspaces/my-app/` — Conductor（外部，绑定挂载；将 `my-app` 替换为你的仓库文件夹名称）

## Conductor 环境变量

- 避免依赖 Conductor 特有的环境变量（例如 `CONDUCTOR_PORT`、`CONDUCTOR_WORKSPACE_PATH`）来进行 Coast 内部的运行时配置。Coast 会独立管理端口、工作区路径和服务发现——请改用 Coastfile `[ports]` 和 `coast exec`。
