# Shared Service Groups

> **Beta.** Shared Service Groups are fully functional but the CLI flags, `Coastfile.shared_service_groups` schema, and on-disk artifact layout may change in future releases.

A Shared Service Group (SSG) is a singleton Docker-in-Docker container on your host that runs infrastructure services (Postgres, Redis, MongoDB, ...) shared across multiple Coast projects. Each project's [Coastfile](../coastfiles/README.md) opts into any SSG service with a single line -- `from_group = true` -- instead of inlining the service under `[shared_services]` on the host Docker daemon.

Where does this fit in the Coast world? The existing [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) pattern runs one infrastructure container per project on the host, claiming canonical host ports. That does not scale across many projects -- two projects declaring `postgres:5432` cannot run at the same time. An SSG replaces those per-project inline services with a single group that every project points at, dynamic host ports under the hood, canonical ports inside each Coast.

## Why an SSG

Without an SSG, every project that needs Postgres starts its own Postgres on the host daemon, claiming host port 5432. Two projects collide on that port the moment you try to run them side by side.

```text
Host Docker daemon (without SSG)
|
+-- project-a-shared-services / postgres  (binds host :5432)
+-- project-b-shared-services / postgres  (fails: port in use)
+-- project-c-shared-services / postgres  (fails: port in use)
```

With an SSG, one Postgres runs inside the `coast-ssg` singleton. Every project's Coast reaches it at the canonical name `postgres:5432`, and the SSG's published host port is allocated dynamically.

```text
Host Docker daemon
|
+-- coast-ssg (singleton DinD)
|     +-- postgres  (inner :5432, host :54201 -- dynamic)
|     +-- redis     (inner :6379, host :54202 -- dynamic)
|
+-- project-a / dev-1   --> resolves postgres:5432 via socat --> host :54201
+-- project-b / dev-1   --> resolves postgres:5432 via socat --> host :54201
+-- project-c / dev-1   --> resolves postgres:5432 via socat --> host :54201
```

The contract inside each Coast does not change. Your compose file still points at `postgres:5432`. Your app container still resolves the service by name. The only thing different is what lives on the other side of `host.docker.internal:{dynamic}` -- the SSG's inner Postgres instead of a host-daemon Postgres.

## Quick Start

Declare the SSG once. Any directory that contains a `Coastfile.shared_service_groups` is a valid starting point -- the build discovery rules mirror `coast build`.

```toml
# Coastfile.shared_service_groups
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

Build and run it:

```bash
coast ssg build
coast ssg run
coast ssg ps
```

Point a project's Coastfile at it:

```toml
# Coastfile in any project
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true
inject = "env:DATABASE_URL"
```

Then `coast build && coast run dev-1` as usual. The SSG auto-starts if it's not already running and your app container sees `postgres:5432` along with a `DATABASE_URL` pointing at it.

## Reference

| Page | What it covers |
|------|----------------|
| [Building](BUILDING.md) | `coast ssg build`, the `Coastfile.shared_service_groups` discovery rules, and the on-disk artifact layout |
| [Lifecycle](LIFECYCLE.md) | `run`/`stop`/`start`/`restart`/`rm`/`ps`/`logs`/`exec`/`ports`, auto-start on `coast run`, and the `--force` gates for remote consumers |
| [Volumes](VOLUMES.md) | Host bind mounts, symmetric-path mechanics, inner named volumes, permission caveats, the `coast ssg doctor` command, and the host-volume migration recipe |
| [Consuming](CONSUMING.md) | `from_group = true`, forbidden fields, drift detection between `coast build` and `coast run`, `auto_create_db`, `inject`, and remote coasts |
| [Checkout](CHECKOUT.md) | `coast ssg checkout` / `uncheckout` for host-side access to canonical ports, displacement semantics, and stop/start behavior |
| [Pinning](PINNING.md) | `coast ssg checkout-build` / `uncheckout-build` / `show-pin` -- pin a consumer project to a specific SSG build so rebuilds don't drift the consumer |
| [CLI](CLI.md) | One-line summary of every `coast ssg` subcommand |

## See Also

- [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) -- the inline-per-project pattern that SSG generalizes across projects
- [Shared Services Coastfile reference](../coastfiles/SHARED_SERVICES.md) -- consumer-side TOML syntax including `from_group`
- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) -- full schema for `Coastfile.shared_service_groups`
- [Ports](../concepts_and_terminology/PORTS.md) -- canonical vs dynamic ports, which the SSG routing layer reuses
