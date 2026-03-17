# T3 Code

[T3 Code](https://github.com/pingdotgg/t3code) 是 Ping 提供的一个开源编码代理 harness。每个工作区都是一个 git worktree，存储在 `~/.t3/worktrees/<project-name>/` 中，并检出到一个命名分支上。

由于这些 worktree 位于项目根目录之外，Coast 需要显式配置才能发现并挂载它们。

## 设置

将 `~/.t3/worktrees/<project-name>` 添加到 `worktree_dir`。T3 Code 会将 worktree 嵌套在每个项目对应的子目录下，因此路径必须包含项目名称。在下面的示例中，`my-app` 必须与您的仓库在 `~/.t3/worktrees/` 下的实际文件夹名称一致。

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.t3/worktrees/my-app"]
```

Coast 会在运行时展开 `~`，并将任何以 `~/` 或 `/` 开头的路径视为外部路径。详情请参见 [Worktree Directories](../coastfiles/WORKTREE_DIR.md)。

更改 `worktree_dir` 后，现有实例必须被**重新创建**，绑定挂载才能生效:

```bash
coast rm my-instance
coast build
coast run my-instance
```

worktree 列表会立即更新（Coast 会读取新的 Coastfile），但要分配到 T3 Code 的 worktree，则需要容器内存在该绑定挂载。

## Coast 的作用

- **绑定挂载** — 在容器创建时，Coast 会将 `~/.t3/worktrees/<project-name>` 挂载到容器中的 `/host-external-wt/{index}`。
- **发现** — `git worktree list --porcelain` 的作用域是仓库级别，因此只会显示属于当前项目的 worktree。
- **命名** — T3 Code worktree 使用命名分支，因此它们会在 Coast UI 和 CLI 中以分支名显示。
- **分配** — `coast assign` 会从外部绑定挂载路径重新挂载 `/workspace`。
- **Gitignored 同步** — 在主机文件系统上使用绝对路径运行，无需绑定挂载即可工作。
- **孤儿检测** — git watcher 会递归扫描外部目录，并通过 `.git` gitdir 指针进行过滤。如果 T3 Code 删除了某个工作区，Coast 会自动取消该实例的分配。

## 示例

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.t3/worktrees/my-app"]
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
- `~/.t3/worktrees/my-app/` — T3 Code（外部，绑定挂载；请将 `my-app` 替换为您的仓库文件夹名称）

## 限制

- Coast 可以发现并挂载 T3 Code worktree，但不会创建或删除它们。
- 由 `coast assign` 创建的新 worktree 始终位于本地 `default_worktree_dir` 中，而不会位于外部目录中。
- 避免依赖 T3 Code 特定的环境变量来进行 Coasts 内部的运行时配置。Coast 会独立管理端口、工作区路径和服务发现——请改用 Coastfile `[ports]` 和 `coast exec`。
