# Shared Services

The `[shared_services.*]` sections define infrastructure services -- databases, caches, message brokers -- that a Coast project consumes. There are two flavors:

- **Inline** -- declare `image`, `ports`, `env`, `volumes` directly in the consumer Coastfile. Coast starts a host-side container and routes the consumer's app traffic to it. Best for solo projects with one consumer instance, or for very lightweight services.
- **From a Shared Service Group (`from_group = true`)** -- the service lives in the project's [Shared Service Group](../shared_service_groups/README.md) (a separate DinD container declared in `Coastfile.shared_service_groups`). The consumer Coastfile only opts in. Best when you want secret extraction, host-side checkout to canonical ports, or you run multiple Coast projects on this host that each need the same canonical port (an SSG keeps Postgres on inner `:5432` without binding host 5432, so two projects can coexist).

The two halves of this page document each flavor in turn.

For how shared services work at runtime, lifecycle management, and troubleshooting, see [Shared Services (concept)](../concepts_and_terminology/SHARED_SERVICES.md).

---

## Inline shared services

Each inline service is a named TOML section under `[shared_services]`. The `image` field is required; everything else is optional.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image` (required)

The Docker image to run on the host daemon.

### `ports`

List of ports the service exposes. Coast accepts either bare container ports or
Docker Compose-style `"HOST:CONTAINER"` mappings.

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- A bare integer like `6379` is shorthand for `"6379:6379"`.
- A mapped string like `"5433:5432"` publishes the shared service on host port
  `5433` while keeping it reachable inside Coasts at `service-name:5432`.
- Host and container ports must both be non-zero.

### `volumes`

Docker volume bind strings for persisting data. These are host-level Docker volumes, not Coast-managed volumes.

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
```

### `env`

Environment variables passed to the service container.

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_DB = "mydb" }
```

### `auto_create_db`

When `true`, Coast automatically creates a per-instance database inside the shared service for each Coast instance. Defaults to `false`.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
auto_create_db = true
```

### `inject`

Injects the shared service connection info into Coast instances as an environment variable or file. Uses the same `env:NAME` or `file:/path` format as [secrets](SECRETS.md).

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
inject = "env:DATABASE_URL"
```

### Lifecycle

Inline shared services start automatically when the first Coast instance that references them runs. They keep running across `coast stop` and `coast rm` -- removing an instance does not affect shared service data. Only `coast shared rm` stops and removes the service.

Per-instance databases created by `auto_create_db` also survive instance deletion. Use `coast shared-services rm` to remove the service and its data entirely.

### Inline examples

#### Postgres, Redis, and MongoDB

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_MULTIPLE_DATABASES = "dev_db,test_db" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis_data:/data"]

[shared_services.mongodb]
image = "mongo:latest"
ports = [27017]
volumes = ["infra_mongodb_data:/data/db"]
env = { MONGO_INITDB_ROOT_USERNAME = "myapp", MONGO_INITDB_ROOT_PASSWORD = "myapp_pass" }
```

#### Minimal shared Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### Host/container mapped Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = ["5433:5432"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### Auto-created databases

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

---

## Shared services from a Shared Service Group

For projects that want a structured shared-infra setup -- multiple worktrees, host-side checkout, SSG-native secrets, virtual ports across SSG rebuilds -- declare the services once in a [`Coastfile.shared_service_groups`](SHARED_SERVICE_GROUPS.md) and reference them from the consumer Coastfile with `from_group = true`:

```toml
[shared_services.postgres]
from_group = true

# Optional per-consumer overrides:
inject = "env:DATABASE_URL"
# auto_create_db = false    # overrides the SSG service's default
```

The TOML key (`postgres` in this example) must match a service declared in the project's `Coastfile.shared_service_groups`. The SSG referenced here is **always the consumer project's own SSG** (named `<project>-ssg`, where `<project>` is the consumer's `[coast].name`).

### Forbidden fields with `from_group = true`

The following fields are rejected at parse time because the SSG is the single source of truth:

- `image`
- `ports`
- `env`
- `volumes`

Any of these alongside `from_group = true` produces:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports, env, volumes.
```

### Allowed per-consumer overrides

- `inject` -- the env-var or file path through which the connection string is exposed. Different consumer Coastfiles may expose the same SSG Postgres under different env-var names.
- `auto_create_db` -- whether Coast should create a per-instance database inside this service at `coast run` time. Overrides the SSG service's own `auto_create_db` value.

### Missing-service error

If you reference a name that isn't declared in the project's `Coastfile.shared_service_groups`, `coast build` fails:

```text
error: shared service 'postgres' has from_group = true but no service named 'postgres' is declared in Coastfile.shared_service_groups for project 'my-app'.
```

### When to choose `from_group` over inline

| Need | Inline | `from_group` |
|---|---|---|
| Single Coast project on this host, no secrets | Either works; inline is simpler | OK |
| Multiple worktrees / consumer instances of the **same** project sharing one Postgres | Works (siblings share one host container) | Works |
| **Two different Coast projects** on this host that each declare the same canonical port (e.g. both want Postgres on 5432) | Collides on host port; cannot run both concurrently | Required (each project's SSG owns its own inner Postgres without binding host 5432) |
| Want host-side `psql localhost:5432` via `coast ssg checkout` | -- | Required |
| Need build-time secret extraction for the service (`POSTGRES_PASSWORD` from a keychain, etc.) | -- | Required (see [SSG Secrets](../shared_service_groups/SECRETS.md)) |
| Stable consumer routing across rebuilds (virtual ports) | -- | Required (see [SSG Routing](../shared_service_groups/ROUTING.md)) |

For the full SSG architecture, see [Shared Service Groups](../shared_service_groups/README.md). For the consumer-side experience including auto-start, drift detection, and remote consumers, see [Consuming](../shared_service_groups/CONSUMING.md).

---

## See Also

- [Shared Services (concept)](../concepts_and_terminology/SHARED_SERVICES.md) -- runtime architecture for both flavors
- [Shared Service Groups](../shared_service_groups/README.md) -- the SSG concept overview
- [Coastfile: Shared Service Groups](SHARED_SERVICE_GROUPS.md) -- the SSG-side Coastfile schema
- [Consuming an SSG](../shared_service_groups/CONSUMING.md) -- detailed walkthrough of `from_group = true` semantics
