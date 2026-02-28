# Volumes

The `[volumes.*]` sections control how named Docker volumes are handled across Coast instances. Each volume is configured with a strategy that determines whether instances share data or get their own independent copy.

For the broader picture of data isolation in Coast — including shared services as an alternative — see [Volumes](../concepts_and_terminology/VOLUMES.md).

## Defining a volume

Each volume is a named TOML section under `[volumes]`. Three fields are required:

- **`strategy`** — `"isolated"` or `"shared"`
- **`service`** — the compose service name that uses this volume
- **`mount`** — the container mount path for the volume

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"
```

## Strategies

### `isolated`

Each Coast instance gets its own independent volume. Data is not shared between instances. Volumes are created on `coast run` and deleted on `coast rm`.

```toml
[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"
```

This is the right choice for most database volumes — each instance gets a clean slate and can mutate data freely without affecting other instances.

### `shared`

All Coast instances use a single Docker volume. Any data written by one instance is visible to all others.

```toml
[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

Shared volumes are never deleted by `coast rm`. They persist until you remove them manually.

Coast prints a warning at build time if you use `shared` on a volume attached to a database-like service. Sharing a single database volume across multiple concurrent instances can cause corruption. If you need shared databases, use [shared services](SHARED_SERVICES.md) instead.

Good uses for shared volumes: dependency caches (Go modules, npm cache, pip cache), build artifact caches, and other data where concurrent writes are safe or unlikely.

## Snapshot seeding

Isolated volumes can be seeded from an existing Docker volume at instance creation time using `snapshot_source`. The source volume's data is copied into the new isolated volume, which then diverges independently.

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "db"
mount = "/var/lib/postgresql/data"
```

`snapshot_source` is only valid with `strategy = "isolated"`. Setting it on a shared volume is an error.

This is useful when you want each Coast instance to start with a realistic dataset copied from your host development database, but you want instances to be free to mutate that data without affecting the source or each other.

## Examples

### Isolated databases, shared dependency cache

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"

[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

### Snapshot-seeded full stack

Each instance starts with a copy of your host's existing database volumes, then diverges independently.

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "infra_redis_data"
service = "redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
snapshot_source = "infra_mongodb_data"
service = "mongodb"
mount = "/data/db"
```

### Test runner with clean databases per instance

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
service = "mongodb"
mount = "/data/db"
```
