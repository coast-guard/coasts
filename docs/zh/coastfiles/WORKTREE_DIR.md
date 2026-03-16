# Worktree 目录

`[coast]` 中的 `worktree_dir` 字段控制 git worktree 的存放位置。Coast 使用 git worktree 为每个实例提供代码库在不同分支上的独立副本，而无需复制整个仓库。

## 语法

`worktree_dir` 接受单个字符串或字符串数组:

```toml
# Single directory (default)
worktree_dir = ".worktrees"

# Multiple directories
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

省略时，默认值为 `".worktrees"`。

## 路径类型

### 相对路径

不以 `~/` 或 `/` 开头的路径会相对于项目根目录解析。这是最常见的情况，不需要特殊处理——它们位于项目目录内，并通过标准的 `/host-project` 绑定挂载自动在 Coast 容器内可用。

```toml
worktree_dir = ".worktrees"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

### 波浪线路径（外部）

以 `~/` 开头的路径会展开为用户的主目录，并被视为**外部** worktree 目录。Coast 会添加单独的绑定挂载，以便容器可以访问它们。

```toml
worktree_dir = ["~/.codex/worktrees", ".worktrees"]
```

这就是你与那些会在项目根目录之外创建 worktree 的工具集成的方式，例如 OpenAI Codex（它总是在 `$CODEX_HOME/worktrees` 创建 worktree）。

### 绝对路径（外部）

以 `/` 开头的路径也会被视为外部路径，并获得其自己的绑定挂载。

```toml
worktree_dir = ["/shared/worktrees", ".worktrees"]
```

## 外部目录的工作方式

当 Coast 遇到外部 worktree 目录（波浪线路径或绝对路径）时，会发生三件事:

1. **容器绑定挂载** —— 在容器创建时（`coast run`），解析后的主机路径会被绑定挂载到容器中的 `/host-external-wt/{index}`，其中 `{index}` 是该路径在 `worktree_dir` 数组中的位置。这使外部文件在容器内可访问。

2. **项目过滤** —— 外部目录可能包含多个项目的 worktree。Coast 使用 `git worktree list --porcelain`（其作用域天然限定在当前仓库）来仅发现属于此项目的 worktree。git watcher 还会通过读取每个 worktree 的 `.git` 文件并检查其 `gitdir:` 指针是否解析回当前仓库来验证归属关系。

3. **工作区重新挂载** —— 当你将 `coast assign` 到外部 worktree 时，Coast 会将 `/workspace` 从外部绑定挂载路径重新挂载，而不是使用通常的 `/host-project/{dir}/{name}`。

## 外部 worktree 的命名

已检出分支的外部 worktree 会显示为其分支名，与本地 worktree 相同。

处于 **detached HEAD** 的外部 worktree（Codex 中很常见）会使用它们在外部目录中的相对路径显示。例如，位于 `~/.codex/worktrees/a0db/coastguard-platform` 的 Codex worktree 会在 UI 和 CLI 中显示为 `a0db/coastguard-platform`。

## `default_worktree_dir`

控制当 Coast 创建**新的** worktree 时使用哪个目录（例如，当你分配一个尚不存在对应 worktree 的分支时）。默认使用 `worktree_dir` 中的第一个条目。

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
default_worktree_dir = ".worktrees"
```

外部目录永远不会用于创建新的 worktree——Coast 总是在本地（相对）目录中创建 worktree。只有当你想覆盖默认值（第一个条目）时，才需要 `default_worktree_dir` 字段。

## 示例

### Codex 集成

OpenAI Codex 会在 `~/.codex/worktrees/{hash}/{project-name}` 创建 worktree。要让这些 worktree 在 Coast 中可见并可分配:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

添加后，Codex 的 worktree 会出现在 checkout 模态框和 `coast ls` 输出中。你可以将一个 Coast 实例分配到某个 Codex worktree，以便在完整开发环境中运行其代码。

注意:添加外部目录后，必须重新创建容器（`coast run`）绑定挂载才会生效。仅重启现有实例是不够的。

### Claude Code 集成

Claude Code 会在项目内部的 `.claude/worktrees/` 创建 worktree。由于这是一个相对路径（位于项目根目录内），它与任何其他本地 worktree 目录一样工作——不需要外部挂载:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

### 三者一起使用

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

## 实时读取 Coastfile

你在 Coastfile 中对 `worktree_dir` 的更改会立即对 worktree **列表显示** 生效（API 和 git watcher 读取的是磁盘上的实时 Coastfile，而不仅仅是缓存的构建产物）。但是，外部**绑定挂载**仅在容器创建时建立，因此如果新增了外部目录，你需要重新创建实例，才能使其可被挂载。
