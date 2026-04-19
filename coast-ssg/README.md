# coast-ssg

Singleton Shared Service Group (SSG) runtime. Runs host-level
infrastructure services (postgres, redis, mongodb, ...) inside a single
Docker-in-Docker container so multiple Coast projects can share them
without host-port collisions.

A consumer Coastfile opts in per-service with:

```toml
[shared_services.postgres]
from_group = true
# Optional per-project overrides:
inject = "env:DATABASE_URL"
```

Everything else (image, inner port, env, volumes) comes from the active
`Coastfile.shared_service_groups` build. One SSG per host in v1.

> Status: scaffolding only (Phase 0). No functional code yet. See
> [`DESIGN.md` §0 "Implementation progress"](./DESIGN.md#0-implementation-progress).

## What this crate is

A library crate. `coast-daemon` depends on it. `coast-cli` routes
`coast ssg <verb>` through the daemon. `coast-service` does **not**
depend on this crate — remote coasts reach the local SSG via the
pre-existing `SharedServicePortForward` protocol plus reverse SSH
tunnels. See `DESIGN.md §20`.

## Where things live

Every SSG-related file in the repository has `ssg` in its path.
`Glob **/*ssg*` returns the complete feature map.

| Concern                            | Path                                                           |
|------------------------------------|----------------------------------------------------------------|
| Design doc / source of truth       | [`./DESIGN.md`](./DESIGN.md)                                   |
| Parser for `Coastfile.shared_service_groups` | [`src/coastfile/`](./src/coastfile/)                 |
| Build artifact + manifest          | [`src/build/`](./src/build/)                                   |
| Runtime (DinD + inner compose)     | [`src/runtime/`](./src/runtime/)                               |
| Symmetric-path bind mounts         | [`src/runtime/bind_mounts.rs`](./src/runtime/bind_mounts.rs)   |
| Nested-exec `auto_create_db`       | [`src/runtime/auto_create_db.rs`](./src/runtime/auto_create_db.rs) |
| SQLite state extension             | [`src/state.rs`](./src/state.rs)                               |
| Filesystem paths (`~/.coast/ssg/`) | [`src/paths.rs`](./src/paths.rs)                               |
| Host canonical-port checkout       | [`src/port_checkout.rs`](./src/port_checkout.rs)               |
| Daemon integration entrypoints     | [`src/daemon_integration.rs`](./src/daemon_integration.rs)     |
| Remote reverse-tunnel pair helpers | [`src/remote_tunnel.rs`](./src/remote_tunnel.rs)               |
| Protocol types                     | `coast-core/src/protocol/ssg.rs` (Phase 1)                     |
| Consumer Coastfile `from_group`    | `coast-core/src/coastfile/` (Phase 1)                          |
| Daemon request handler             | `coast-daemon/src/handlers/ssg.rs` (Phase 1+)                  |
| Daemon run-integration adapter     | `coast-daemon/src/handlers/run/ssg_integration.rs` (Phase 4)   |
| CLI command                        | `coast-cli/src/commands/ssg.rs` (Phase 1+)                     |
| User docs (runtime)                | `docs/concepts_and_terminology/SHARED_SERVICE_GROUPS.md` (Phase 8) |
| User docs (TOML)                   | `docs/coastfiles/SHARED_SERVICE_GROUPS.md` (Phase 8)           |

## Naming conventions

| Concern                                | Convention                              |
|----------------------------------------|-----------------------------------------|
| Crate                                  | `coast-ssg`                             |
| Rust types                             | `Ssg*` prefix (e.g. `SsgCoastfile`)     |
| Files outside this crate               | must contain `ssg` in filename          |
| CLI verb                               | `coast ssg <subcommand>`                |
| Protocol enums                         | `SsgRequest::{Build, Run, ...}`         |
| DB tables                              | `ssg`, `ssg_services`, `ssg_port_checkouts` |
| Filesystem root                        | `~/.coast/ssg/`                         |
| User file name                         | `Coastfile.shared_service_groups`       |
| Consumer Coastfile field               | `[shared_services.<name>] from_group = true` |
| Docs file names                        | `SHARED_SERVICE_GROUPS.md`              |
| Log target                             | `coast::ssg`                            |
| Terms in code comments                 | "SSG" or "Shared Service Group" — never bare "group" |

## Discoverability contract

The feature is built to be grokkable from zero context in 2-3 tool
calls. The rules are normative — they appear in `DESIGN.md §4.2` and
apply to every PR that touches SSG.

- `Glob **/*ssg*` from the repo root returns the complete feature map.
- `rg '\bSsg'` returns every type the feature introduces.
- `Read coast-ssg/README.md` + `Read coast-ssg/DESIGN.md` = full mental
  model, no other files required.

### Banned patterns

- Inlining `if has_ssg_ref { ... }` logic in a file whose name does not
  contain `ssg`. Factor it into an `*ssg*` adapter file instead.
- Using the bare term "group" in SSG-specific code. Always qualify:
  `SsgServiceConfig`, not `GroupServiceConfig`.
- Adding any SSG-aware code to `coast-service`. Remote SSG is an
  explicit non-goal; the contract lives entirely on the local side.
- Adding a non-`Ssg*` field to a cross-cutting type (`Coastfile`,
  `RunRequest`, `StateDb`) without a doc comment that mentions "SSG"
  or "Shared Service Group" — keeps `rg` giving the full picture.

## Remote coasts

`coast-service` never learns that SSGs exist. A remote coast that
references an SSG service uses the already-existing
`SharedServicePortForward` protocol: the remote's inner compose
container resolves `postgres:5432` via `extra_hosts` to
`host.docker.internal:5432` on the remote VM, which is the far end of
a reverse SSH tunnel (`ssh -R`). The only SSG-aware logic is the local
daemon's rewrite of the tunnel pair's local side from "canonical port"
to "SSG dynamic port". See `DESIGN.md §20` for the full flow and
interop rules.

## Current status

See [`DESIGN.md` §0 Implementation progress](./DESIGN.md#0-implementation-progress).
Every PR that advances a phase ticks the boxes it closes in the same
commit.

## Design doc

The complete design, phased plan, and normative rules live in
[`DESIGN.md`](./DESIGN.md). Read it before writing code.
