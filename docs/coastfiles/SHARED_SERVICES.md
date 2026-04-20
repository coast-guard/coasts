# Shared Services

The `[shared_services.*]` sections define infrastructure services — databases, caches, message brokers — that run on the host Docker daemon rather than inside individual Coast containers. Multiple Coast instances connect to the same shared service over a bridge network.

For how shared services work at runtime, lifecycle management, and troubleshooting, see [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md).

## Defining a shared service

Each shared service is a named TOML section under `[shared_services]`. The `image` field is required; everything else is optional.

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

## Lifecycle

Shared services start automatically when the first Coast instance that references them runs. They keep running across `coast stop` and `coast rm` — removing an instance does not affect shared service data. Only `coast shared rm` stops and removes a shared service.

Per-instance databases created by `auto_create_db` also survive instance deletion. Use `coast shared-services rm` to remove the service and its data entirely.

## When to use shared services vs volumes

Use shared services when multiple Coast instances need to talk to the same database server (e.g. a shared Postgres where each instance gets its own database). Use [volume strategies](VOLUMES.md) when you want to control how a compose-internal service's data is shared or isolated.

## Opting into a Shared Service Group

If you want a single Postgres / Redis / MongoDB to back **multiple projects** rather than a single project, declare it once in a [`Coastfile.shared_service_groups`](SHARED_SERVICE_GROUPS.md) and reference it from each consumer with `from_group = true`:

```toml
[shared_services.postgres]
from_group = true

# Optional per-project overrides:
inject = "env:DATABASE_URL"
# auto_create_db = false    # overrides the SSG service's default
```

The TOML key (`postgres` in this example) must match a service declared in the active Shared Service Group.

### Forbidden fields with `from_group = true`

With `from_group = true`, the following fields are rejected at parse time because the SSG is the single source of truth:

- `image`
- `ports`
- `env`
- `volumes`

Any of these fields present alongside `from_group = true` produces:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports.
```

### Allowed per-consumer overrides

- `inject` -- the env-var or file name through which the connection string is exposed. Different projects may expose the same SSG Postgres under different env-var names.
- `auto_create_db` -- whether Coast should create a per-instance database inside this service at `coast run` time. Overrides the SSG service's own `auto_create_db` value.

### Missing-service error

If you reference a name that does not exist in the active SSG, `coast run` fails with:

```text
error: shared service 'postgres' references the shared service group, but no service 'postgres' exists in ~/.coast/ssg/latest.
```

For the full concept, architecture, and operational guide, see [Shared Service Groups](../shared_service_groups/README.md) in the docs.

## Examples

### Postgres, Redis, and MongoDB

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

### Minimal shared Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

### Host/container mapped shared Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = ["5433:5432"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

### Shared services with auto-created databases

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```
