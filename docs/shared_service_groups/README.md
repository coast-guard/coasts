# Shared Service Groups

A Shared Service Group (SSG) is a Docker-in-Docker container that runs your project's infrastructure services -- Postgres, Redis, MongoDB, anything you'd otherwise put under `[shared_services]` -- in one place, separately from the [Coast](../concepts_and_terminology/COASTS.md) instances that consume it. Every Coast project gets its own SSG, named `<project>-ssg`, declared by a `Coastfile.shared_service_groups` sibling to the project's `Coastfile`.

Each consumer instance (`dev-1`, `dev-2`, ...) connects to its project's SSG via stable virtual ports, so SSG rebuilds don't churn the consumers. Inside each Coast, the contract is unchanged: `postgres:5432` resolves to your shared Postgres, the application code doesn't know anything is special.

## Why an SSG

The original [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) pattern starts one infrastructure container on the host Docker daemon and shares it across every consumer instance in the project. That works fine for one project. The trouble starts when you have **two different projects** that each declare a Postgres on `5432`: both projects try to bind the same host port and the second one fails.

```text
Without an SSG (cross-project host-port collision):

Host Docker daemon
+-- cg-coasts-postgres            (project "cg" binds host :5432)
+-- filemap-coasts-postgres       (project "filemap" tries :5432 -- FAILS)
+-- cg-coasts-dev-1               --> cg-coasts-postgres
+-- cg-coasts-dev-2               --> cg-coasts-postgres   (siblings share fine)
```

SSGs solve this by hoisting each project's infrastructure into its own DinD. Postgres still listens on canonical `:5432` -- but inside the SSG, not on the host. The SSG container is published on an arbitrary dynamic host port, and a daemon-managed virtual-port socat (in the `42000-43000` band) bridges consumer traffic to it. Two projects can each have a Postgres on canonical 5432 because neither one binds host 5432:

```text
With an SSG (per project, no cross-project collision):

Host Docker daemon
+-- cg-ssg                        (project "cg" -- DinD)
|     +-- postgres                (inner :5432, host dyn 54201, vport 42000)
|     +-- redis                   (inner :6379, host dyn 54202, vport 42001)
+-- filemap-ssg                   (project "filemap" -- DinD, no collision)
|     +-- postgres                (inner :5432, host dyn 54250, vport 42002)
|     +-- redis                   (inner :6379, host dyn 54251, vport 42003)
+-- cg-coasts-dev-1               --> hg-internal:42000 --> cg-ssg postgres
+-- cg-coasts-dev-2               --> hg-internal:42000 --> cg-ssg postgres
+-- filemap-coasts-dev-1          --> hg-internal:42002 --> filemap-ssg postgres
```

Each project's SSG owns its own data, its own image versions, and its own secrets. The two never share state, never compete for ports, and never see each other's data. Inside each consumer Coast, the contract is unchanged: app code dials `postgres:5432` and gets its own project's Postgres -- the routing layer (see [Routing](ROUTING.md)) does the rest.

## Quick Start

A `Coastfile.shared_service_groups` is a sibling of the project's `Coastfile`. The project name comes from `[coast].name` in the regular Coastfile -- you don't repeat it.

```toml
# Coastfile.shared_service_groups
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["pg_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_DB = "app_dev" }
auto_create_db = true

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]

# Optional: extract secrets from your environment, keychain, or 1Password
# at build time and inject them into the SSG at run time. See SECRETS.md.
[secrets.pg_password]
extractor = "env"
inject = "env:POSTGRES_PASSWORD"
var = "MY_PG_PASSWORD"
```

Build it and run it:

```bash
coast ssg build       # parse, pull images, extract secrets, write artifact
coast ssg run         # start <project>-ssg, materialize secrets, compose up
coast ssg ps          # show service status
```

Point a consumer Coast at it:

```toml
# Coastfile in the same project
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true
inject = "env:DATABASE_URL"

[shared_services.redis]
from_group = true
```

Then `coast build && coast run dev-1`. The SSG auto-starts if it isn't already running. Inside `dev-1`'s app container, `postgres:5432` resolves to the SSG's Postgres and `$DATABASE_URL` is set to a canonical connection string.

## Reference

| Page | What it covers |
|---|---|
| [Building](BUILDING.md) | `coast ssg build` end-to-end, the per-project artifact layout, secret extraction, the `Coastfile.shared_service_groups` discovery rules, and how to lock a project to a specific build |
| [Lifecycle](LIFECYCLE.md) | `run` / `start` / `stop` / `restart` / `rm` / `ps` / `logs` / `exec`, the per-project `<project>-ssg` container, auto-start on `coast run`, and `coast ssg ls` for cross-project listing |
| [Routing](ROUTING.md) | Canonical / dynamic / virtual ports, the host socat layer, the full hop-by-hop chain from app to inner service, and remote-consumer symmetric tunnels |
| [Volumes](VOLUMES.md) | Host bind mounts, symmetric paths, inner named volumes, permissions, the `coast ssg doctor` command, and migrating an existing host volume into the SSG |
| [Consuming](CONSUMING.md) | `from_group = true`, allowed and forbidden fields, conflict detection, `auto_create_db`, `inject`, and remote consumers |
| [Secrets](SECRETS.md) | `[secrets.<name>]` in the SSG Coastfile, the build-time extractor pipeline, run-time injection via `compose.override.yml`, and the `coast ssg secrets clear` verb |
| [Checkout](CHECKOUT.md) | `coast ssg checkout` / `uncheckout` for binding the SSG's canonical ports on the host so anything on your host (psql, redis-cli, IDE) can reach them |
| [CLI](CLI.md) | One-line summary of every `coast ssg` subcommand |
| [REMOTE_DESIGN.md](../../coast-ssg/REMOTE_DESIGN.md) | Remote SSGs (per `(project, remote)`), the `coast ssg point` runtime pointer, and the importer/exporter framework. Pre-implementation; Phase R-0 through R-7. Implementation companion: [REMOTE_SSG_SERVICE_DESIGN.md](../../coast-service/REMOTE_SSG_SERVICE_DESIGN.md). |

## See Also

- [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) -- the inline-per-instance pattern that SSG generalizes
- [Shared Services Coastfile reference](../coastfiles/SHARED_SERVICES.md) -- consumer-side TOML syntax including `from_group`
- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) -- full schema for `Coastfile.shared_service_groups`
- [Ports](../concepts_and_terminology/PORTS.md) -- canonical vs dynamic ports
