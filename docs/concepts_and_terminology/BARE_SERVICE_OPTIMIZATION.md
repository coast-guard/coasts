# Bare Service Optimization

[Bare services](BARE_SERVICES.md) run as plain processes inside the Coast container. Without Docker layers or image caches, startup and branch-switch performance depends on how you structure your `install` commands, caching, and assign strategies.

## Fast Install Commands

The `install` field runs before the service starts and again on every `coast assign`. If `install` unconditionally runs `make` or `yarn install`, every branch switch pays the full install cost even when nothing changed.

**Use conditional checks to skip work when possible:**

```toml
[services.web]
install = "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)"
command = "cd /workspace && yarn dev:web"
```

The `test -f` guard skips the install if `node_modules` already exists. On the first run or after a cache miss, it runs the full install. On subsequent assigns where dependencies have not changed, it completes instantly.

For compiled binaries, check if the output exists:

```toml
[services.zoekt]
install = "cd /workspace && (test -f bin/zoekt-webserver || make zoekt)"
command = "cd /workspace && ./bin/zoekt-webserver -index .sourcebot/index -rpc"
```

## Cache Directories Across Worktrees

When Coast switches a bare-service instance to a new worktree, the `/workspace` mount changes to a different directory. Build artifacts like `node_modules` or compiled binaries are left behind in the old worktree. The `cache` field tells Coast to preserve specified directories across switches:

```toml
[services.web]
install = "cd /workspace && yarn install"
command = "cd /workspace && yarn dev"
cache = ["node_modules"]

[services.api]
install = "cd /workspace && make build"
command = "cd /workspace && ./bin/api-server"
cache = ["bin"]
```

Cached directories are backed up before the worktree remount and restored afterward. This means `yarn install` runs incrementally instead of from scratch, and compiled binaries survive branch switches.

## Isolate Per-Instance Directories with private_paths

Some tools create directories in the workspace that contain per-process state: lock files, build caches, or PID files. When multiple Coast instances share the same workspace (same branch, no worktree), these directories collide.

The classic example is Next.js, which takes a lock at `.next/dev/lock` on startup. A second Coast instance sees the lock and refuses to start.

`private_paths` gives each instance its own isolated directory for the specified paths:

```toml
[coast]
name = "my-app"
private_paths = ["packages/web/.next"]
```

Each instance gets a per-instance overlay mount at that path. The lock files, build caches, and Turbopack state are fully isolated. No code changes needed.

Use `private_paths` for any directory where concurrent instances writing to the same files causes problems: `.next`, `.turbo`, `.parcel-cache`, PID files, or SQLite databases.

## Connecting to Shared Services

When you use [shared services](SHARED_SERVICES.md) for databases or caches, the shared containers run on the host Docker daemon, not inside the Coast. Bare services running inside the Coast cannot reach them via `localhost`.

Use `host.docker.internal` instead:

```toml
[services.web]
command = "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

You can also use [secrets](../coastfiles/SECRETS.md) to inject connection strings as environment variables:

```toml
[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"
```

Compose services inside the Coast do not have this issue. Coast automatically routes shared service hostnames through a bridge network for compose containers. This only affects bare services.

## Inline Environment Variables

Bare service commands inherit environment variables from the Coast container, including anything set via `.env` files, secrets, and inject. But sometimes you need to override a specific variable for a single service without changing shared config files.

Prefix the command with inline assignments:

```toml
[services.web]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

Inline variables take precedence over everything else. This is useful for:

- Setting `AUTH_URL` to the [dynamic port](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) so auth redirects work on non-checked-out instances
- Overriding `DATABASE_URL` to point at a shared service via `host.docker.internal`
- Setting service-specific flags without modifying shared `.env` files in the workspace

## Assign Strategies for Bare Services

Choose the right [assign strategy](../coastfiles/ASSIGN.md) based on how each service picks up code changes:

| Strategy | When to use | Examples |
|---|---|---|
| `hot` | The service has a file watcher that detects changes automatically after the worktree remount | Next.js (HMR), Vite, webpack, nodemon, tsc --watch |
| `restart` | The service loads code at startup and does not watch for changes | Compiled Go binaries, Rails, Java servers |
| `none` | The service does not depend on workspace code or uses a separate index | Database servers, Redis, search indexes |

```toml
[assign]
default = "none"

[assign.services]
web = "hot"
backend = "hot"
zoekt = "none"
```

Setting the default to `none` means infrastructure services are never touched on branch switch. Only the services that care about code changes get restarted or rely on hot reload.

## See Also

- [Bare Services](BARE_SERVICES.md) - the full bare services reference
- [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md) - general performance tuning including `exclude_paths` and `rebuild_triggers`
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - using `WEB_DYNAMIC_PORT` and related variables in commands
