# Coastfile Types

A single project can have multiple Coastfiles for different use cases. Each variant is called a "type." Types let you compose configurations that share a common base but differ in what services run, how volumes are handled, or whether services autostart.

## How Types Work

The naming convention is `Coastfile` for the default and `Coastfile.{type}` for variants. The suffix after the dot becomes the type name:

- `Coastfile` -- default type
- `Coastfile.test` -- test type
- `Coastfile.snap` -- snapshot type
- `Coastfile.light` -- lightweight type

You build and run typed Coasts with `--type`:

```bash
coast build --type test
coast run test-1 --type test
coast exec test-1 -- go test ./...
```

## extends

A typed Coastfile inherits from a parent via `extends`. Everything from the parent is merged in. The child only needs to specify what it overrides or adds.

```toml
[coast]
extends = "Coastfile"
```

This avoids duplicating your entire configuration for each variant. The child inherits all [ports](PORTS.md), [secrets](SECRETS.md), [volumes](VOLUMES.md), [shared services](SHARED_SERVICES.md), [assign strategies](ASSIGN.md), setup commands, and [MCP](MCP_SERVERS.md) configurations from the parent. Anything the child defines takes precedence over the parent.

## [unset]

Removes specific items inherited from the parent by name. You can unset `ports`, `shared_services`, `secrets`, and `volumes`.

```toml
[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]
```

This is how a test variant drops shared services (so databases run inside the Coast with isolated volumes) and removes ports it does not need.

## [omit]

Strips compose services entirely from the build. Omitted services are removed from the compose file and do not run inside the Coast at all.

```toml
[omit]
services = ["redis", "backend", "mailhog", "web"]
```

Use this to exclude services that are irrelevant to the variant's purpose. A test variant might only keep the database, migrations, and test runner.

## autostart

Controls whether `docker compose up` runs automatically when the Coast starts. The default is `true`.

```toml
[coast]
extends = "Coastfile"
autostart = false
```

Set `autostart = false` for variants where you want to run specific commands manually rather than bringing up the full stack. This is common for test runners -- you create the Coast, then use [`coast exec`](EXEC_AND_DOCKER.md) to run individual test suites.

## Common Patterns

### Test variant

A `Coastfile.test` that keeps only what is needed for running tests:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]

[omit]
services = ["redis", "backend", "mailhog", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[assign]
default = "none"
[assign.services]
test-runner = "rebuild"
migrations = "rebuild"
```

Each test Coast gets its own clean database. No ports are exposed because tests talk to services over the internal compose network. `autostart = false` means you trigger test runs manually with `coast exec`.

### Snapshot variant

A `Coastfile.snap` that seeds each Coast with a copy of the host's existing database volumes:

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis"]

[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "my_project_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "my_project_redis_data"
service = "redis"
mount = "/data"
```

Shared services are unset so databases run inside each Coast. `snapshot_source` seeds the isolated volumes from existing host volumes at build time. After creation, each instance's data diverges independently.

### Lightweight variant

A `Coastfile.light` that strips the project down to the minimum for a specific workflow -- perhaps just a backend service and its database for rapid iteration.

## Independent Build Pools

Each type has its own `latest-{type}` symlink and its own 5-build auto-pruning pool:

```bash
coast build              # updates latest, prunes default builds
coast build --type test  # updates latest-test, prunes test builds
coast build --type snap  # updates latest-snap, prunes snap builds
```

Building a `test` type does not affect `default` or `snap` builds. Pruning is completely independent per type.

## Running Typed Coasts

Instances created with `--type` are tagged with their type. You can have instances of different types running simultaneously for the same project:

```bash
coast run dev-1                    # default type
coast run test-1 --type test       # test type
coast run snapshot-1 --type snap   # snapshot type

coast ls
# All three appear, each with their own type, ports, and volume strategy
```

This is how you can have a full dev environment running alongside isolated test runners and snapshot-seeded instances, all for the same project, all at the same time.
