# Coastfile.shared_service_groups

`Coastfile.shared_service_groups` is a typed Coastfile that declares the services the Shared Service Group (SSG) will run. Exactly one SSG Coastfile is active at a time, host-wide. Multiple projects then reference its services with `[shared_services.<name>] from_group = true` in their own Coastfiles.

For the concept, lifecycle, volumes, and consumer wiring, see the [Shared Service Groups documentation](../shared_service_groups/README.md).

## Discovery

`coast ssg build` finds the file using the same rules as `coast build`:

- Default: look for `Coastfile.shared_service_groups` or `Coastfile.shared_service_groups.toml` in the current working directory. Both forms are equivalent; the `.toml` variant wins when both exist.
- `-f <path>` / `--file <path>` points at an arbitrary file.
- `--working-dir <dir>` decouples the project root from the Coastfile location.
- `--config '<toml>'` accepts inline TOML for scripted flows.

## Accepted Sections

Only `[ssg]`, `[shared_services.<name>]`, and `[unset]` are accepted. Any other top-level key (`[coast]`, `[ports]`, `[services]`, `[volumes]`, `[secrets]`, `[assign]`, `[omit]`, ...) is rejected at parse.

`[ssg] extends = "<path>"` and `[ssg] includes = ["<path>", ...]` are supported for composition. See [Inheritance](#inheritance) below.

## `[ssg]`

Top-level SSG configuration.

```toml
[ssg]
runtime = "dind"
```

### `runtime` (optional)

Container runtime for the outer SSG DinD. `dind` is the only supported value today; the field is optional and defaults to `dind`.

## `[shared_services.<name>]`

One block per service. The TOML key (`postgres`, `redis`, ...) becomes the service name that consumer Coastfiles reference.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

### `image` (required)

The Docker image to run inside the SSG's inner Docker daemon. Any public or private image the host can pull is accepted.

### `ports`

Container ports the service listens on. **Bare integers only.**

```toml
ports = [5432]
ports = [5432, 5433]
```

- A `"HOST:CONTAINER"` mapping (`"5432:5432"`) is **rejected**. SSG host publications are always dynamic -- you never pick the host port.
- An empty array (or the field omitted entirely) is allowed. Sidecars without exposed ports are fine.

Each port becomes a `PUBLISHED:CONTAINER` mapping on the outer DinD at `coast ssg run` time, where `PUBLISHED` is a dynamically-allocated host port.

### `env`

Flat string map forwarded verbatim into the inner service container's environment.

```toml
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "app" }
```

Env values are **not** captured in the manifest. Only the keys are recorded, matching the safety posture of `coast build`.

### `volumes`

Array of Docker-Compose-style volume strings. Each entry is one of:

```toml
volumes = [
    "/var/coast-data/postgres:/var/lib/postgresql/data",   # host bind mount
    "pg_wal:/var/lib/postgresql/wal",                       # inner named volume
]
```

**Host bind mount** -- source starts with `/`. The bytes live on the real host filesystem. Both the outer DinD and the inner service bind the **same host path string**. See [Volumes -> Symmetric-Path Plan](../shared_service_groups/VOLUMES.md#the-symmetric-path-plan).

**Inner named volume** -- source is a Docker volume name (no `/`). The volume lives inside the SSG's inner Docker daemon. Persists across SSG restarts; opaque to the host.

Rejected at parse:

- Relative paths (`./data:/...`).
- `..` components.
- Container-only volumes (no source).
- Duplicate targets within a single service.

### `auto_create_db`

When `true`, the daemon creates a `{instance}_{project}` database inside this service for every consumer Coast that runs. Only applies to recognized database images (Postgres, MySQL). Defaults to `false`.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

A consumer Coastfile can override this value per project -- see [Consuming -> auto_create_db](../shared_service_groups/CONSUMING.md#auto_create_db).

### `inject` (not allowed)

`inject` is **not** valid on SSG service definitions. Injection is a consumer-side concern (different projects may want the same SSG Postgres exposed under different env-var names). See [Coastfile: Shared Services](SHARED_SERVICES.md#inject) for the consumer-side `inject` semantics.

## Inheritance

SSG Coastfiles support the same `extends` / `includes` / `[unset]` mechanism as regular Coastfiles. See [Coastfile Inheritance](INHERITANCE.md) for the shared mental model; this section documents the SSG-specific shape.

### `[ssg] extends` -- pull in a parent Coastfile

```toml
[ssg]
extends = "Coastfile.ssg-base"

[shared_services.postgres]
image = "postgres:17-alpine"
```

The parent file is resolved relative to the child's parent directory. The `.toml` tie-break applies (the parser tries `Coastfile.ssg-base.toml` first, then plain `Coastfile.ssg-base`). Absolute paths are also accepted.

### `[ssg] includes` -- merge fragment files

```toml
[ssg]
includes = ["dev-seed.toml", "extra-caches.toml"]

[shared_services.postgres]
image = "postgres:16-alpine"
```

Fragments are merged in order before the including file itself. Fragment paths are resolved relative to the including file's parent directory (no `.toml` tie-break -- fragments are typically named exactly).

**Fragments cannot themselves use `extends` or `includes`.** They must be self-contained. This keeps the dependency graph a tree rooted at a single `from_file` call.

### Merge semantics

- **`[ssg]` scalars** (`runtime`) -- child wins when present, else inherit.
- **`[shared_services.*]`** -- by-name replace. If parent and child both define `postgres`, the child's entry fully replaces the parent's (whole-entry replacement, not field-level merge). Parent services not re-declared by the child are inherited.
- **Load order** -- `extends` parent loads first, then each `includes` fragment in order, then the top-level file itself. Later layers win on collision.

### `[unset]` -- drop inherited services

```toml
[ssg]
extends = "Coastfile.ssg-base"

[unset]
shared_services = ["mongodb"]
```

Removes named entries **after** the merge, so a child can selectively drop something the parent provides. Only the `shared_services` key is supported -- no other collection exists in the SSG schema.

Standalone SSG Coastfiles may technically contain `[unset]`, but it is silently ignored (matches regular Coastfile behavior: unset only applies when the file participates in inheritance).

### Cycles

Direct cycles (`A` extends `B` extends `A`, or `A` extends itself) are hard-errored with `circular extends/includes dependency detected: '<path>'`. Diamond inheritance (two separate paths that both end at the same parent) is allowed -- the visit-set is per-recursion and pops on return.

### `[omit]` is not applicable

Regular Coastfiles support `[omit]` to strip services / volumes from the compose file. The SSG has no compose file to strip -- it generates inner compose from `[shared_services.*]` entries directly. Use `[unset]` to drop inherited services instead.

### Inline `--config` rejects `extends` / `includes`

`coast ssg build --config '<toml>'` cannot resolve a parent path because there is no on-disk location to anchor relative paths to. Passing `extends` / `includes` in inline TOML hard-errors with `extends and includes require file-based parsing`. Use `-f <file>` or `--working-dir <dir>` instead.

### Build artifact is the flattened form

`coast ssg build` writes a standalone TOML to `~/.coast/ssg/builds/<id>/ssg-coastfile.toml`. The artifact contains the post-inheritance merged result with no `extends`, `includes`, or `[unset]` directives, so the build can be inspected or re-run without the parent / fragment files being present. The `build_id` hash also reflects the flattened form, so a parent-only change invalidates the cache correctly.

## Example

Minimal Postgres + Redis:

```toml
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["/var/coast-data/redis:/data"]
```

## See Also

- [Shared Service Groups](../shared_service_groups/README.md) -- concept overview
- [SSG Building](../shared_service_groups/BUILDING.md) -- what `coast ssg build` does with this file
- [SSG Volumes](../shared_service_groups/VOLUMES.md) -- volume declaration shapes, permissions, and the host-volume migration recipe
- [Coastfile: Shared Services](SHARED_SERVICES.md) -- consumer-side `from_group = true` syntax
- [Coastfile Inheritance](INHERITANCE.md) -- the shared `extends` / `includes` / `[unset]` mental model
