# Assign

The `[assign]` section controls what happens to services inside a Coast instance when you switch branches with `coast assign`. Each service can be configured with a different strategy depending on whether it needs a full rebuild, a restart, a hot-reload, or nothing at all.

For how `coast assign` and `coast unassign` work at runtime, see [Assign](../concepts_and_terminology/ASSIGN.md).

## `[assign]`

### `default`

The default action applied to all services on branch switch. Defaults to `"restart"` if the entire `[assign]` section is omitted.

- **`"none"`** — do nothing. The service keeps running as-is. Good for databases and caches that don't depend on code.
- **`"hot"`** — the code is already live-mounted via the [filesystem](../concepts_and_terminology/FILESYSTEM.md), so the service picks up changes automatically (e.g. via a file watcher or hot-reload). No container restart needed.
- **`"restart"`** — restart the service container. Use when the service reads code at startup but doesn't need a full image rebuild.
- **`"rebuild"`** — rebuild the service's Docker image and restart. Required when code is baked into the image via `COPY` or `ADD` in the Dockerfile.

```toml
[assign]
default = "none"
```

### `[assign.services]`

Per-service overrides. Each key is a compose service name, and the value is one of the four actions above.

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

This lets you leave databases and caches untouched (`"none"` via the default) while rebuilding or restarting only the services that depend on the code that changed.

### `[assign.rebuild_triggers]`

File patterns that force a rebuild for specific services, even if their default action is something lighter. Each key is a service name, and the value is a list of file paths or patterns.

```toml
[assign]
default = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json", "package-lock.json"]
```

### `exclude_paths`

A list of paths to exclude from worktree sync during `coast assign`. Useful in large monorepos where certain directories are irrelevant to the services running in the Coast and would otherwise slow down the assign operation.

```toml
[assign]
default = "none"
exclude_paths = ["apps/ide", "apps/extension", "apps/ide-extension"]

[assign.services]
backend = "hot"
web = "hot"
```

## Examples

### Rebuild app, leave everything else alone

When your app service bakes code into its Docker image but your databases are independent of code changes:

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

### Hot-reload frontend and backend

When both services use file watchers (e.g. Next.js dev server, Go air, nodemon) and code is live-mounted:

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

### Per-service rebuild with triggers

The API service normally just restarts, but if `Dockerfile` or `package.json` changed, it rebuilds:

```toml
[assign]
default = "none"

[assign.services]
api = "restart"
worker = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json"]
```

### Full rebuild for everything

When all services bake code into their images:

```toml
[assign]
default = "rebuild"
```
