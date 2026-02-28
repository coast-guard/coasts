# Assign and Unassign

Assign and unassign control which worktree a Coast instance is pointed at. See [Filesystem](FILESYSTEM.md) for how worktree switching works at the mount level.

## Assign

`coast assign` switches a Coast instance to a specific worktree. Coast creates the worktree if it does not already exist, updates the code inside the Coast, and restarts services according to the configured assign strategy.

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

After assigning, `dev-1` is running the `feature/oauth` branch with all its services up.

## Unassign

`coast unassign` switches a Coast instance back to the project root (your main/master branch). The worktree association is removed and the Coast returns to running off the primary repository.

```text
coast unassign dev-1

┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘
```

## Assign Strategies

When a Coast is assigned to a new worktree, each service needs to know how to handle the code change. You configure this per-service in your [Coastfile](COASTFILE_TYPES.md) under `[assign]`:

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

The available strategies are:

- **none** — do nothing. Use this for services that do not change between branches, such as Postgres or Redis.
- **hot** — swap the filesystem only. The service stays running and picks up changes via mount propagation and file watchers (e.g., a dev server with hot reload).
- **restart** — restart the service container. Use this for interpreted services that just need a process restart. This is the default.
- **rebuild** — rebuild the service image and restart. Use this when the branch change affects the `Dockerfile` or build-time dependencies.

You can also specify rebuild triggers so that a service only rebuilds when specific files change:

```toml
[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
```

If none of the trigger files changed between branches, the service skips the rebuild even if the strategy is set to `rebuild`.

## Deleted Worktrees

If an assigned worktree is deleted, the `coastd` daemon automatically unassigns that instance back to the main Git repository root.

---

> **Tip: Reducing assign latency in large codebases**
>
> Under the hood, Coast runs `git ls-files` whenever a worktree is mounted or unmounted. In large codebases or repositories with many files, this can add noticeable latency to assign and unassign operations.
>
> If parts of your codebase do not need to be rebuilt between assigns, you can tell Coast to skip them using `exclude_paths` in your Coastfile:
>
> ```toml
> [assign]
> default = "restart"
> exclude_paths = ["docs", "scripts", "test-fixtures"]
> ```
>
> Paths listed in `exclude_paths` are ignored during the file diff, which can significantly speed up assign times.
