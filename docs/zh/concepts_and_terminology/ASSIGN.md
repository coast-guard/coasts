# 分配与取消分配

分配与取消分配用于控制某个 Coast 实例指向哪个 worktree。关于 worktree 切换在挂载层是如何工作的，请参见 [Filesystem](FILESYSTEM.md)。

## 分配

`coast assign` 会将某个 Coast 实例切换到指定的 worktree。如果 worktree 尚不存在，Coast 会创建它，更新 Coast 内部的代码，并根据配置的分配策略重启相关服务。

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

分配完成后，`dev-1` 将运行 `feature/oauth` 分支，并启动其所有服务。

## 取消分配

`coast unassign` 会将某个 Coast 实例切换回项目根目录（你的 main/master 分支）。worktree 关联将被移除，Coast 将恢复为基于主仓库运行。

```text
coast unassign dev-1

┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘
```

## 分配策略

当某个 Coast 被分配到新的 worktree 时，每个服务都需要知道如何处理代码变更。你可以在 [Coastfile](COASTFILE_TYPES.md) 的 `[assign]` 下为每个服务进行配置:

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

可用的策略有:

- **none** — 不执行任何操作。用于分支之间不会发生变化的服务，例如 Postgres 或 Redis。
- **hot** — 仅切换文件系统。服务保持运行，并通过挂载传播与文件监视器（例如支持热重载的开发服务器）来感知变更。
- **restart** — 重启服务容器。用于解释型服务，只需要重启进程即可。这是默认值。
- **rebuild** — 重新构建服务镜像并重启。当分支切换会影响 `Dockerfile` 或构建时依赖时使用。

你也可以指定重新构建触发器，使得服务仅在特定文件发生变化时才会重新构建:

```toml
[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
```

如果分支之间这些触发文件都没有变化，即使策略设置为 `rebuild`，该服务也会跳过重新构建。

## 已删除的 Worktree

如果某个已分配的 worktree 被删除，`coastd` 守护进程会自动将该实例取消分配并切回主 Git 仓库根目录。

---

> **提示:在大型代码库中降低分配延迟**
>
> 在底层实现中，Coast 会在每次挂载或卸载 worktree 时运行 `git ls-files`。在大型代码库或包含大量文件的仓库中，这可能会为分配与取消分配操作带来明显的延迟。
>
> 如果你的代码库中有些部分不需要在分配之间重新构建，你可以在 Coastfile 中通过 `exclude_paths` 告诉 Coast 跳过它们:
>
> ```toml
> [assign]
> default = "restart"
> exclude_paths = ["docs", "scripts", "test-fixtures"]
> ```
>
> `exclude_paths` 中列出的路径在文件差异比较期间会被忽略，这可以显著加快分配耗时。
