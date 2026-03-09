# Performance Optimizations

Coast is designed to make branch switching fast, but in large monorepos the default behavior can still introduce latency. This page covers the levers available in your Coastfile and, more importantly, which parts of `coast assign` they actually affect.

## Why Assign Can Be Slow

`coast assign` does several things when switching a Coast to a new worktree:

```text
coast assign dev-1 --worktree feature/payments

  1. classify services and optional rebuild-trigger diff
  2. stop affected services
  3. create git worktree (if new)
  4. bootstrap gitignored files into the worktree (first assign only)
  5. remount /workspace
  6. recreate/restart containers
  7. rebuild images for services using "rebuild"
  8. wait for healthy
```

The biggest variable costs are usually the **first-time gitignored bootstrap**, **container restarts**, and **image rebuilds**. The optional branch diff used for rebuild triggers is much cheaper, but it can still add up if you point it at broad trigger sets.

### Gitignored File Bootstrap

When a worktree is created for the first time, Coast bootstraps selected gitignored files from the project root into that worktree.

The sequence is:

1. Run `git ls-files --others --ignored --exclude-standard` on the host to enumerate ignored files.
2. Filter out common heavy directories plus any configured `exclude_paths`.
3. Run `rsync --files-from` with `--link-dest` so the selected files are hardlinked into the worktree instead of copied byte-for-byte.
4. Record the successful bootstrap in internal worktree metadata so later assigns to the same worktree can skip it.

If `rsync` is unavailable, Coast falls back to a `tar` pipeline.

Large directories such as `node_modules`, `.git`, `dist`, `target`, `.next`, `.nuxt`, `.cache`, `.worktrees`, and `.coasts` are excluded automatically. Large dependency directories are expected to be handled by service caches or volumes rather than by this generic bootstrap step.

Because the file list is generated up front, `rsync` is working from a targeted list rather than blindly crawling the entire repository. Even so, repos with very large ignored-file sets can still pay a noticeable one-time bootstrap cost when a worktree is first created. If you ever need to refresh that bootstrap manually, run `coast assign --force-sync`.

### Rebuild-Trigger Diff

Coast only computes a branch diff when `[assign.rebuild_triggers]` is configured. In that case it runs:

```bash
git diff --name-only <previous>..<worktree>
```

The result is used to downgrade a service from `rebuild` to `restart` when none of its trigger files changed.

This is much narrower than the old "diff every tracked file on every assign" model. If you do not configure rebuild triggers, there is no branch diff step here at all.

`exclude_paths` does not currently change this diff. Keep your trigger lists focused on true build-time inputs such as Dockerfiles, lockfiles, and package manifests.

## `exclude_paths` — The Main Lever for New Worktrees

The `exclude_paths` option in your Coastfile tells Coast to skip entire directory trees while building the gitignored bootstrap file list for a new worktree.

```toml
[assign]
default = "none"
exclude_paths = [
    "docs",
    "scripts",
    "test-fixtures",
    "apps/mobile",
]
```

Files under excluded paths are still present in the worktree if Git tracks them. Coast just avoids spending time enumerating and hardlinking ignored files under those trees during the first-time bootstrap.

This is most impactful when your repo root contains large ignored directories that your running services do not care about: unrelated apps, vendored caches, test fixtures, generated docs, and other heavy trees.

If you are repeatedly assigning to the same already-synced worktree, `exclude_paths` matters less because the bootstrap is skipped. In that case, service restart/rebuild choices become the dominant factor.

### Choosing What to Exclude

Start by profiling your ignored files:

```bash
git ls-files --others --ignored --exclude-standard | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

If you also want a view of the tracked layout for rebuild-trigger tuning, use:

```bash
git ls-files | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

**Keep** directories that:
- Contain source code mounted into running services
- Contain shared libraries imported by those services
- Contain generated files or caches that your runtime actually needs on first boot
- Are referenced in `[assign.rebuild_triggers]`

**Exclude** directories that:
- Belong to apps or services not running in your Coast
- Contain documentation, scripts, CI configs, or tooling unrelated to runtime
- Hold large ignored caches that are already preserved elsewhere, such as dedicated service caches or shared volumes

### Example: Monorepo With Multiple Apps

A monorepo with many top-level directories, but only a subset matters to the services running in this Coast:

```text
  13,000  bookface/         ← active
   7,000  ycinternal/       ← active
     850  shared/           ← used by both
   3,800  .yarn/            ← excludable
   2,500  startupschool/    ← excludable
     500  misc/             ← excludable
     300  ycapp/            ← excludable
     ...  (12 more dirs)    ← excludable
```

```toml
[assign]
default = "none"
exclude_paths = [
    ".yarn",
    "startupschool",
    "misc",
    "ycapp",
    "apply",
    "cli",
    "deploy",
    "lambdas",
    # ... any other directories not needed by active services
]
```

This keeps the first-time worktree bootstrap focused on the directories the running services actually need instead of spending time on unrelated ignored trees.

## Trim Inactive Services From `[assign.services]`

If your `COMPOSE_PROFILES` only starts a subset of services, remove inactive services from `[assign.services]`. Coast evaluates the assign strategy for every listed service, and restarting or rebuilding a service that is not running is wasted work.

```toml
# Bad — restarts services that aren't running
[assign.services]
web = "restart"
api = "restart"
mobile-api = "restart"   # not in COMPOSE_PROFILES
batch-worker = "restart"  # not in COMPOSE_PROFILES

# Good — only services that are actually running
[assign.services]
web = "restart"
api = "restart"
```

The same applies to `[assign.rebuild_triggers]` — remove entries for services that are not active.

## Use `"hot"` Where Possible

The `"hot"` strategy skips the container restart entirely. The [filesystem remount](FILESYSTEM.md) swaps the code under `/workspace` and the service's file watcher (Vite, webpack, nodemon, air, etc.) picks up changes automatically.

```toml
[assign.services]
web = "hot"        # Vite/webpack dev server with HMR
api = "restart"    # Rails/Go — needs a process restart
```

`"hot"` is faster than `"restart"` because it avoids the container stop/start cycle. Use it for any service that runs a dev server with file watching. Reserve `"restart"` for services that load code at startup and do not watch for changes (most Rails, Go, and Java apps).

## Use `"rebuild"` With Triggers

If a service's default strategy is `"rebuild"`, every branch switch rebuilds the Docker image — even if nothing affecting the image changed. Add `[assign.rebuild_triggers]` to gate the rebuild on specific files:

```toml
[assign.services]
worker = "rebuild"

[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json", "package-lock.json"]
```

If none of the trigger files changed between branches, Coast skips the rebuild and falls back to a restart instead. This avoids expensive image builds on routine code changes.

## Summary

| Optimization | Impact | Affects | When to use |
|---|---|---|---|
| `exclude_paths` | High | first-time gitignored bootstrap | Repos with large ignored trees your Coast does not need |
| Remove inactive services | Medium | service restart/recreate | When `COMPOSE_PROFILES` limits which services run |
| `"hot"` strategy | High | container restart | Services with file watchers (Vite, webpack, nodemon, air) |
| `rebuild_triggers` | High | image rebuilds + optional branch diff | Services using `"rebuild"` that only need it for infra changes |

If new worktrees are slow to assign for the first time, start with `exclude_paths`. If repeat assigns are slow, focus on `hot` vs `restart`, trim inactive services, and keep `rebuild_triggers` tight.
