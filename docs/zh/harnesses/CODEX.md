# Codex

[Codex](https://developers.openai.com/codex/app/worktrees/) 会在 `$CODEX_HOME/worktrees`（通常是 `~/.codex/worktrees`）创建 worktree。每个 worktree 都位于一个不透明的哈希目录下，例如 `~/.codex/worktrees/a0db/project-name`，以 detached HEAD 状态开始，并会根据 Codex 的保留策略自动清理。

摘自 [Codex docs](https://developers.openai.com/codex/app/worktrees/):

> 我可以控制 worktree 的创建位置吗？
> 目前不可以。Codex 会在 `$CODEX_HOME/worktrees` 下创建 worktree，以便能够一致地管理它们。

由于这些 worktree 位于项目根目录之外，Coast 需要显式配置才能发现并挂载它们。

## Setup

将 `~/.codex/worktrees` 添加到 `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

Coast 会在运行时展开 `~`，并将任何以 `~/` 或 `/` 开头的路径视为外部路径。详情请参见 [Worktree Directories](../coastfiles/WORKTREE_DIR.md)。

更改 `worktree_dir` 后，必须**重新创建**现有实例，才能使 bind mount 生效:

```bash
coast rm my-instance
coast build
coast run my-instance
```

worktree 列表会立即更新（Coast 会读取新的 Coastfile），但要分配到 Codex worktree，则需要容器内存在 bind mount。

## What Coast does

- **Bind mount** -- 在创建容器时，Coast 会将 `~/.codex/worktrees` 挂载到容器中的 `/host-external-wt/{index}`。
- **Discovery** -- `git worktree list --porcelain` 的作用域是仓库级别，因此即使该目录包含许多项目的 worktree，也只会显示属于当前项目的 Codex worktree。
- **Naming** -- detached HEAD worktree 会显示为其在外部目录中的相对路径（`a0db/my-app`、`eca7/my-app`）。基于分支的 worktree 会显示分支名称。
- **Assign** -- `coast assign` 会从外部 bind mount 路径重新挂载 `/workspace`。
- **Gitignored sync** -- 在主机文件系统上使用绝对路径运行，无需 bind mount 也可工作。
- **Orphan detection** -- git watcher 会递归扫描外部目录，并通过 `.git` gitdir 指针进行过滤。如果 Codex 删除了某个 worktree，Coast 会自动取消该实例的分配。

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
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

- `.worktrees/` -- Coast 管理的 worktree
- `.claude/worktrees/` -- Claude Code（本地，无需特殊处理）
- `~/.codex/worktrees/` -- Codex（外部，使用 bind mount 挂载）

## Limitations

- Coast 可以发现并挂载 Codex worktree，但不会创建或删除它们。
- Codex 可能随时清理 worktree。Coast 的 orphan detection 可以妥善处理这种情况。
- 由 `coast assign` 创建的新 worktree 总是放在本地 `default_worktree_dir` 中，绝不会放在外部目录中。
