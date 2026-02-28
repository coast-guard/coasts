# Volume Topology

Coast provides three volume strategies that control how data-heavy services (databases, caches, etc.) store and share their data across Coast instances. Choosing the right strategy depends on how much isolation you need and how much overhead you can tolerate.

## Shared Services

[Shared services](SHARED_SERVICES.md) run on your host Docker daemon, outside of any Coast container. Services like Postgres, MongoDB, and Redis stay on the host machine and Coast instances route their calls back to the host over a bridge network.

```text
Host machine
  |
  +--> Postgres (host daemon, existing volume)
  +--> Redis (host daemon, existing volume)
  |
  +--> Coast: dev-1  --connects to--> host Postgres, host Redis
  +--> Coast: dev-2  --connects to--> host Postgres, host Redis
```

There is no data isolation between instances — every Coast talks to the same database. In return you get:

- Lighter Coast instances since they do not run their own database containers.
- Your existing host volumes are reused directly, so any data you already have is available immediately.
- MCP integrations that connect to your local database continue to work out of the box.

This is configured in your [Coastfile](COASTFILE_TYPES.md) under `[shared_services]`.

## Shared Volumes

Shared volumes mount a single Docker volume that is shared across all Coast instances. The services themselves (Postgres, Redis, etc.) run inside each Coast container, but they all read and write to the same underlying volume.

```text
Coast: dev-1  --mounts--> shared volume "my-project-postgres"
Coast: dev-2  --mounts--> shared volume "my-project-postgres"
```

This isolates your Coast data from whatever is on your host machine, but instances still share data with each other. This is useful when you want a clean separation from your host development environment without the overhead of per-instance volumes.

```toml
[volumes.postgres_data]
strategy = "shared"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

## Isolated Volumes

Isolated volumes give each Coast instance its own independent volume. No data is shared between instances or with the host. Each instance starts empty (or from a snapshot — see below) and diverges independently.

```text
Coast: dev-1  --mounts--> volume "dev-1-postgres"
Coast: dev-2  --mounts--> volume "dev-2-postgres"
```

This is the best choice for projects that are integration-test heavy and need true volume isolation between parallel environments. The tradeoff is slower startup and larger Coast builds since each instance maintains its own copy of the data.

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

## Snapshotting

Both the shared and isolated strategies start with empty volumes by default. If you want instances to start with a copy of an existing host volume, set `snapshot_source` to the name of the Docker volume to copy from:

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

The snapshot is taken at [build time](BUILDS.md). After creation, each instance's volume diverges independently — mutations do not propagate back to the source or to other instances.

Coast does not yet support runtime snapshotting (e.g., snapshotting a volume from a running instance). This is planned for a future release.
