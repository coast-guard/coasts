# 分配与取消分配

分配（assign）与取消分配（unassign）用于控制某个 Coast 实例指向哪个 worktree。关于 worktree 切换在挂载层面如何工作，请参见 [Filesystem](FILESYSTEM.md)。

## 分配

`coast assign` 将某个 Coast 实例切换到指定的 worktree。如果该 worktree 尚不存在，Coast 会创建它，更新 Coast 内部的代码，并根据配置的分配策略重启相关服务。

```bash
coast assign dev-1 --worktree feature/oauth
```

```text
Before:
┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘

coast assign dev-1 --worktree feature/oauth

After:
┌─── dev-1 ──────────────────┐
│  branch: feature/oauth     │
│  worktree: feature/oauth   │
│                            │
│  postgres → skipped (none) │
│  web      → hot swapped    │
│  api      → restarted      │
│  worker   → rebuilt        │
└────────────────────────────┘
```

分配之后，`dev-1` 将运行 `feature/oauth` 分支，并且其所有服务都会启动。

## 取消分配

`coast unassign` 将某个 Coast 实例切换回项目根目录（你的 main/master 分支）。worktree 关联会被移除，Coast 将恢复为从主仓库运行。

```text
coast unassign dev-1

┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘
```

## 分配策略

当某个 Coast 被分配到新的 worktree 时，每个服务都需要知道如何处理代码变更。你可以在 [Coastfile](COASTFILE_TYPES.md) 中的 `[assign]` 为每个服务配置:

```toml
[assign]
default = "restart"

[assign.services]
postgres = "none"
redis = "none"
web = "hot"
worker = "rebuild"
```

```text
coast assign dev-1 --worktree feature/billing

  postgres (strategy: none)    →  skipped, unchanged between branches
  redis (strategy: none)       →  skipped, unchanged between branches
  web (strategy: hot)          →  filesystem swapped, file watcher picks it up
  api (strategy: restart)      →  container restarted
  worker (strategy: rebuild)   →  image rebuilt, container restarted
```

可用策略包括:

- **none** — 不做任何事。用于在不同分支之间不会变化的服务，例如 Postgres 或 Redis。
- **hot** — 仅交换文件系统。服务保持运行，并通过挂载传播与文件监视器拾取变更（例如具备热重载的开发服务器）。
- **restart** — 重启服务容器。用于只需要重启进程的解释型服务。这是默认值。
- **rebuild** — 重新构建服务镜像并重启。用于分支切换会影响 `Dockerfile` 或构建时依赖的情况。

你也可以指定重建触发器，使服务仅在特定文件发生变化时才重建:

```toml
[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
```

如果触发文件在分支之间没有变化，即使策略设置为 `rebuild`，该服务也会跳过重建。

## 已删除的 Worktree

如果已分配的 worktree 被删除，`coastd` 守护进程会自动将该实例取消分配并切回主 Git 仓库根目录。

---

> **提示:在大型代码库中降低分配延迟**
>
> 在底层，对新 worktree 的首次分配会将选定的被 gitignore 忽略的文件引导（bootstrap）到该 worktree 中，并且带有 `[assign.rebuild_triggers]` 的服务可能会运行 `git diff --name-only` 来决定是否需要重建。在大型代码库中，该引导步骤和不必要的重建往往是分配耗时的主要来源。
>
> 在你的 Coastfile 中使用 `exclude_paths` 来缩小被 gitignore 忽略文件的引导范围；对带有文件监视器的服务使用 `"hot"`；并让 `[assign.rebuild_triggers]` 聚焦于真正的构建时输入。如果你需要为已有 worktree 手动刷新被忽略文件的引导内容，请运行 `coast assign --force-sync`。完整指南参见 [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md)。
