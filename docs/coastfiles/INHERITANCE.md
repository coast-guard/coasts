# Inheritance, Types, and Composition

Coastfiles support inheritance (`extends`), fragment composition (`includes`), item removal (`[unset]`), and compose-level stripping (`[omit]`). Together, these let you define a base configuration once and create lean variants for different workflows — test runners, lightweight frontends, snapshot-seeded stacks — without duplicating configuration.

For a higher-level overview of how typed Coastfiles fit into the build system, see [Coastfile Types](../concepts_and_terminology/COASTFILE_TYPES.md) and [Builds](../concepts_and_terminology/BUILDS.md).

## Coastfile types

The base Coastfile is always named `Coastfile`. Typed variants use the naming pattern `Coastfile.{type}`:

- `Coastfile` — the default type
- `Coastfile.light` — type `light`
- `Coastfile.snap` — type `snap`
- `Coastfile.ci.minimal` — type `ci.minimal`

The name `Coastfile.default` is reserved and not allowed. A trailing dot (`Coastfile.`) is also invalid.

Build and run typed variants with `--type`:

```
coast build --type light
coast run test-1 --type light
```

Each type has its own independent build pool. A `--type light` build does not interfere with default builds.

## `extends`

A typed Coastfile can inherit from a parent using `extends` in the `[coast]` section. The parent is fully parsed first, then the child's values are layered on top.

```toml
[coast]
extends = "Coastfile"
```

The value is a relative path to the parent Coastfile, resolved against the child's directory. Chains are supported — a child can extend a parent that itself extends a grandparent:

```
Coastfile                    (base)
  └─ Coastfile.light         (extends Coastfile)
       └─ Coastfile.chain    (extends Coastfile.light)
```

Circular chains (A extends B extends A, or A extends A) are detected and rejected.

### Merge semantics

When a child extends a parent:

- **Scalar fields** (`name`, `runtime`, `compose`, `root`, `worktree_dir`, `autostart`, `primary_port`) — child value wins if present; otherwise inherited from parent.
- **Maps** (`[ports]`, `[egress]`) — merged by key. Child keys override same-named parent keys; parent-only keys are preserved.
- **Named sections** (`[secrets.*]`, `[volumes.*]`, `[shared_services.*]`, `[mcp.*]`, `[mcp_clients.*]`, `[services.*]`) — merged by name. A child entry with the same name fully replaces the parent entry; new names are added.
- **`[coast.setup]`**:
  - `packages` — deduplicated union (child adds new packages, parent packages are kept)
  - `run` — child commands are appended after parent commands
  - `files` — merged by `path` (same path = child's entry replaces parent's)
- **`[inject]`** — `env` and `files` lists are concatenated.
- **`[omit]`** — `services` and `volumes` lists are concatenated.
- **`[assign]`** — entirely replaced if present in the child (not merged field-by-field).
- **`[agent_shell]`** — entirely replaced if present in the child.

### Inheriting the project name

If the child does not set `name`, it inherits the parent's name. This is normal for typed variants — they're variants of the same project:

```toml
# Coastfile
[coast]
name = "my-app"
```

```toml
# Coastfile.light — inherits name "my-app"
[coast]
extends = "Coastfile"
autostart = false
```

You can override `name` in the child if you want the variant to appear as a separate project:

```toml
[coast]
extends = "Coastfile"
name = "my-app-light"
```

## `includes`

The `includes` field merges one or more TOML fragment files into the Coastfile before the file's own values are applied. This is useful for extracting shared configuration (like a set of secrets or MCP servers) into reusable fragments.

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]
```

An included fragment is a TOML file with the same section structure as a Coastfile. It must contain a `[coast]` section (which can be empty) but cannot use `extends` or `includes` itself.

```toml
# extra-secrets.toml
[coast]

[secrets.mongo_uri]
extractor = "env"
var = "MONGO_URI"
inject = "env:MONGO_URI"
```

Merge order when both `extends` and `includes` are present:

1. Parse the parent (via `extends`), recursively
2. Merge each included fragment in order
3. Apply the file's own values (which win over everything)

## `[unset]`

Removes named items from the resolved configuration after all merging is complete. This is how a child removes something it inherited from its parent without having to redefine the entire section.

```toml
[unset]
secrets = ["db_password"]
shared_services = ["postgres", "redis"]
ports = ["postgres", "redis"]
```

Supported fields:

- `secrets` — list of secret names to remove
- `ports` — list of port names to remove
- `shared_services` — list of shared service names to remove
- `volumes` — list of volume names to remove
- `mcp` — list of MCP server names to remove
- `mcp_clients` — list of MCP client names to remove
- `egress` — list of egress names to remove
- `services` — list of bare service names to remove

`[unset]` is applied after the full extends + includes merge chain resolves. It removes items by name from the final merged result.

## `[omit]`

Strips compose services and volumes from the Docker Compose stack that runs inside the Coast. Unlike `[unset]` (which removes Coastfile-level configuration), `[omit]` tells Coast to exclude specific services or volumes when running `docker compose up` inside the DinD container.

```toml
[omit]
services = ["monitoring", "debug-tools", "nginx-proxy"]
volumes = ["keycloak-db-data"]
```

- **`services`** — compose service names to exclude from `docker compose up`
- **`volumes`** — compose volume names to exclude

This is useful when your `docker-compose.yml` defines services you don't need in every Coast variant — monitoring stacks, reverse proxies, admin tools. Rather than maintaining multiple compose files, you use a single compose file and strip what you don't need per variant.

When a child extends a parent, `[omit]` lists are concatenated — the child adds to the parent's omit list.

## Examples

### Lightweight test variant

Extends the base Coastfile, disables autostart, strips shared services, and runs databases isolated per instance:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "backend", "postgres", "redis"]
shared_services = ["postgres", "redis", "mongodb"]

[omit]
services = ["redis", "backend", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[assign]
default = "none"
[assign.services]
backend-test = "rebuild"
migrations = "rebuild"
```

### Snapshot-seeded variant

Removes shared services from the base and replaces them with snapshot-seeded isolated volumes:

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis", "mongodb"]

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

### Typed variant with extra shared services and includes

Extends the base, adds MongoDB, and pulls in extra secrets from a fragment:

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]

[ports]
mongodb = 37017

[shared_services.mongodb]
image = "mongo:7"
ports = [27017]
env = { MONGO_INITDB_ROOT_USERNAME = "dev", MONGO_INITDB_ROOT_PASSWORD = "dev" }

[omit]
services = ["debug-tools"]
```

### Multi-level inheritance chain

Three levels deep: base -> light -> chain.

```toml
# Coastfile.chain
[coast]
extends = "Coastfile.light"

[coast.setup]
run = ["echo 'chain setup appended'"]

[ports]
debug = 39999
```

The resolved configuration starts with the base `Coastfile`, merges `Coastfile.light` on top, then merges `Coastfile.chain` on top of that. Setup `run` commands from all three levels are concatenated in order. Setup `packages` are deduplicated across all levels.

### Omitting services from a large compose stack

Strip services from `docker-compose.yml` that aren't needed for development:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[omit]
services = ["backend-debug", "backend-debug-test", "asynqmon", "postgres-keycloak", "keycloak", "redash-db-init", "redash-init", "redash", "redash-scheduler", "redash-worker", "langfuse-db-init", "langfuse", "nginx-proxy"]
volumes = ["keycloak-db-data"]

[ports]
web = 3000
backend = 8080
```
