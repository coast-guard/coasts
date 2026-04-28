# coast-ssg

Per-project Shared Service Group (SSG) runtime. Runs each project's
infrastructure services (postgres, redis, mongodb, ...) inside a
Docker-in-Docker container — one SSG per project per location — so
multiple Coast projects can run side-by-side without host-port
collisions.

A consumer Coastfile opts in per-service with:

```toml
[shared_services.postgres]
from_group = true
# Optional per-project overrides:
inject = "env:DATABASE_URL"
```

Everything else (image, inner port, env, volumes) comes from the active
`Coastfile.shared_service_groups` build. **One SSG per project per
location** — `local` or a registered remote — switchable at runtime
via `coast ssg point <location>`.

> **Two design docs:** read both before writing code.
>
> 1. [`./DESIGN.md`](./DESIGN.md) — original local-only design (Phases
>    0-33). The historical record. Top banner there points back here.
> 2. [`./REMOTE_DESIGN.md`](./REMOTE_DESIGN.md) — current architecture
>    for remote SSGs, the runtime pointer abstraction, the bidirectional
>    importer/exporter framework (push / pull / transfer), and the same-
>    subnet routing baseline. Phases R-0 through R-7. Coast-service-side
>    implementation companion lives at
>    [`../coast-service/REMOTE_SSG_SERVICE_DESIGN.md`](../coast-service/REMOTE_SSG_SERVICE_DESIGN.md).
>
> Live agent context-recovery path: read this README, then
> `REMOTE_DESIGN.md` (the current truth), and follow its links into
> `DESIGN.md` and `REMOTE_SSG_SERVICE_DESIGN.md` as needed.

## What this crate is

A library crate. `coast-daemon` depends on it for the daemon-side SSG
glue (build, lifecycle, pin/drift, doctor, daemon-state CRUD,
orchestration of `coast ssg export/import/transfer`).

`coast-service` does **not** depend on this crate — the
[`DESIGN.md §4.1`](./DESIGN.md) layering rule is preserved by lifting
the truly shared primitives (host_socat, compose synth, manifest types,
the `SnapshotExporter` / `SnapshotImporter` registry) into
`coast-docker` and `coast-core` (Phase R-0.5). Both daemon and
coast-service consume the lifted primitives without crossing the
crate boundary. See [`REMOTE_DESIGN.md` Ground Rule #3](./REMOTE_DESIGN.md).

`coast-cli` routes `coast ssg <verb>` through the daemon. The daemon
then dispatches local-vs-remote based on the project's pointer.

## Where things live

Every SSG-related file in the repository has `ssg` in its path.
`Glob **/*ssg*` from the repo root returns the complete feature map.

### Top-level docs

| Concern                                | Path                                                                                          |
|----------------------------------------|-----------------------------------------------------------------------------------------------|
| Local-SSG design (history + Phase 33)  | [`./DESIGN.md`](./DESIGN.md)                                                                  |
| Remote SSG + pointer + transfer design | [`./REMOTE_DESIGN.md`](./REMOTE_DESIGN.md)                                                    |
| coast-service-side implementation companion | [`../coast-service/REMOTE_SSG_SERVICE_DESIGN.md`](../coast-service/REMOTE_SSG_SERVICE_DESIGN.md) |
| Remote-coasts spec (sibling to RSSD)   | [`../coast-service/REMOTE_SPEC.md`](../coast-service/REMOTE_SPEC.md)                          |
| User docs index                        | [`../docs/shared_service_groups/README.md`](../docs/shared_service_groups/README.md)          |

### coast-ssg crate (daemon-side)

| Concern                                | Path                                                            |
|----------------------------------------|-----------------------------------------------------------------|
| Parser for `Coastfile.shared_service_groups` (incl. `[ssg].remote`, `[shared_services.<svc>.export/import]`) | [`src/coastfile/`](./src/coastfile/) |
| Build artifact + manifest              | [`src/build/`](./src/build/)                                    |
| Runtime (DinD + inner compose)         | [`src/runtime/`](./src/runtime/)                                |
| Symmetric-path bind mounts             | [`src/runtime/bind_mounts.rs`](./src/runtime/bind_mounts.rs)    |
| Nested-exec `auto_create_db`           | [`src/runtime/auto_create_db.rs`](./src/runtime/auto_create_db.rs) |
| SQLite state extension                 | [`src/state.rs`](./src/state.rs)                                |
| Filesystem paths (`~/.coast/ssg/`)     | [`src/paths.rs`](./src/paths.rs)                                |
| Host canonical-port checkout (legacy local-only verb) | [`src/runtime/port_checkout.rs`](./src/runtime/port_checkout.rs) |
| Daemon integration entrypoints         | [`src/daemon_integration.rs`](./src/daemon_integration.rs)      |
| Reverse-tunnel pair helpers (R-4)      | [`src/remote_tunnel.rs`](./src/remote_tunnel.rs)                |
| `transfer/` orchestration glue (R-1)   | [`src/transfer/`](./src/transfer/) — daemon-side wrapper around the registry |

### Lifted primitives (Phase R-0.5; consumed by both daemon and coast-service)

| Concern                                      | Path                                                       |
|----------------------------------------------|------------------------------------------------------------|
| host_socat supervisor (Docker-network primitive) | `coast-docker/src/host_socat.rs`                       |
| Inner-compose YAML synthesis                 | `coast-docker/src/ssg_compose_synth.rs`                    |
| `SnapshotExporter` / `SnapshotImporter` registry + 6 builtins | `coast-docker/src/ssg_transfer/`        |
| `SsgManifest` and adjacent manifest types    | `coast-core/src/artifact/ssg.rs`                           |

### Cross-crate touchpoints

| Concern                                | Path                                                            |
|----------------------------------------|-----------------------------------------------------------------|
| Protocol types                         | `coast-core/src/protocol/ssg.rs`                                |
| Consumer Coastfile `from_group`        | `coast-core/src/coastfile/`                                     |
| Daemon request handler                 | `coast-daemon/src/handlers/ssg/`                                |
| Daemon run-integration adapter         | `coast-daemon/src/handlers/run/ssg_integration.rs`              |
| Daemon pointer + remote forwarding     | `coast-daemon/src/handlers/ssg/{pointer,remote_forward,remote_tunnel,transfer}.rs` (Phase R-3+) |
| coast-service-side SSG implementation  | `coast-service/src/ssg/{lifecycle,build,transfer,routing,secrets_inject,paths,state}.rs` (Phase R-3+) |
| coast-service HTTP route layer         | `coast-service/src/handlers/ssg.rs` + `ssg_transfer.rs` (Phase R-3+) |
| CLI command                            | `coast-cli/src/commands/ssg.rs`                                 |
| User docs (overview + per-topic)       | [`../docs/shared_service_groups/`](../docs/shared_service_groups/) |

## Naming conventions

| Concern                                | Convention                                                  |
|----------------------------------------|-------------------------------------------------------------|
| Crate                                  | `coast-ssg`                                                 |
| Rust types                             | `Ssg*` prefix (e.g. `SsgCoastfile`)                         |
| Files outside this crate               | must contain `ssg` in filename                              |
| CLI verb                               | `coast ssg <subcommand>`                                    |
| Protocol enums                         | `SsgAction::{Build, Run, Point, Export, Import, ...}`       |
| DB tables (daemon)                     | `ssg`, `ssg_services`, `ssg_virtual_ports`, `ssg_pointers`, `ssg_remote_tunnels`, `ssg_consumer_routing` |
| DB tables (coast-service)              | `ssg`, `ssg_services`, `ssg_virtual_ports`, `ssg_socat_upstreams` |
| Filesystem root (daemon)               | `~/.coast/ssg/`                                             |
| Filesystem root (coast-service)        | `$COAST_SERVICE_HOME/ssg/`                                  |
| User file name                         | `Coastfile.shared_service_groups`                           |
| Consumer Coastfile field               | `[shared_services.<name>] from_group = true`                |
| Docs file names                        | `SHARED_SERVICE_GROUPS.md`, `REMOTE.md`, `POINTER.md`, `IMPORT_EXPORT.md` |
| Log target                             | `coast::ssg`                                                |
| Integration test names (remote)        | `test_remote_ssg_*.sh` (purely-local stays `test_ssg_*.sh`) |
| Test project naming (remote)           | `coast-remote-ssg-*` (purely-local stays `coast-ssg-*`)     |
| Terms in code comments                 | "SSG" or "Shared Service Group" — never bare "group"        |

## Discoverability contract

The feature is built to be grokkable from zero context in 2-3 tool
calls. The rules are normative — they appear in
[`DESIGN.md §4.2`](./DESIGN.md) and apply to every PR that touches
SSG.

- `Glob **/*ssg*` from the repo root returns the complete feature map.
- `rg '\bSsg'` returns every type the feature introduces.
- `Read coast-ssg/README.md` (this file) + `Read coast-ssg/REMOTE_DESIGN.md`
  = full current-truth mental model. `Read coast-ssg/DESIGN.md` for
  local-only history.

### Banned patterns

- Inlining `if has_ssg_ref { ... }` logic in a file whose name does not
  contain `ssg`. Factor it into an `*ssg*` adapter file instead.
- Using the bare term "group" in SSG-specific code. Always qualify:
  `SsgServiceConfig`, not `GroupServiceConfig`.
- **Adding `coast-ssg` as a dependency of `coast-service`.** The
  layering rule from [`DESIGN.md §4.1`](./DESIGN.md) is preserved
  even though remote SSGs are now in scope. Shared primitives belong
  in `coast-docker` or `coast-core`. See
  [`REMOTE_DESIGN.md` Ground Rule #3](./REMOTE_DESIGN.md).
- Adding a non-`Ssg*` field to a cross-cutting type (`Coastfile`,
  `RunRequest`, `StateDb`) without a doc comment that mentions "SSG"
  or "Shared Service Group" — keeps `rg` giving the full picture.
- Adding a `--remote` flag to `coast ssg exec` or `coast ssg logs`.
  Pointer resolution must stay invisible to the user
  ([`REMOTE_DESIGN.md §13.5`](./REMOTE_DESIGN.md)).
- Suppressing clippy lints in Remote-SSG work — no `#[allow(clippy::...)]`,
  no `--allow` flags, no `#[expect(...)]` shortcuts on new code.
  See [`REMOTE_DESIGN.md §27.4`](./REMOTE_DESIGN.md).

## Remote SSGs (current)

A consumer coast routes to its project's SSG at whichever location
the project's pointer resolves to (`local` or a registered remote).
Switching is a runtime verb:

```bash
coast ssg point <local|remote-name>     # repoint live consumers
coast ssg unpoint                        # fall back to Coastfile default
coast ssg pointer                        # show the resolved location
```

Bidirectional data movement:

```bash
coast ssg export postgres --remote prod-vm                 # PULL  (remote -> local)
coast ssg import postgres <snap> --remote staging-vm       # PUSH  (local  -> remote)
coast ssg transfer postgres --from prod-vm --to staging-vm # via daemon's canonical store
```

The data plane between coast-servers is a direct TCP host_socat hop
(no daemon in the data path); the deployment requirement is that
coast-servers share a subnet or are otherwise mutually peered. See
[`REMOTE_DESIGN.md §12 + §16 + Goal #7`](./REMOTE_DESIGN.md) for the
full architecture, the routing decision matrix, and the failure
modes.

The original local-only "remote coasts reach the local SSG via
reverse SSH tunnels" path described in
[`DESIGN.md §20`](./DESIGN.md) is still correct for the
`(remote consumer, local SSG)` quadrant; it's one row in the §12.3
decision matrix in REMOTE_DESIGN.md.

## Current status

Two parallel phase trackers:

- Local-only phases (0-33): see [`DESIGN.md §0`](./DESIGN.md). All
  shipped through Phase 33.
- Remote-SSG phases (R-0 through R-7): see
  [`REMOTE_DESIGN.md §0`](./REMOTE_DESIGN.md). Pre-implementation;
  R-0 in flight (design + scaffolding).

Every PR that advances a phase ticks the boxes it closes in the same
commit, on whichever doc owns that phase.

## Phase-gate discipline

Every PR for either tracker MUST:

- Pass `make test` and `make lint` (`cargo fmt --all -- --check` +
  `cargo clippy --workspace -- -D warnings`).
- Pass every named integration test for the phase via
  `make run-dind-integration TEST=<name>`. Test names are listed in
  the phase's checklist in §0 of the relevant design doc.
- **Add zero clippy suppressions.** Reviewers reject PRs that ship
  any new `#[allow(clippy::...)]` even if every other criterion is
  satisfied. See [`DESIGN.md` Ground Rule #2](./DESIGN.md) and
  [`REMOTE_DESIGN.md` Ground Rule #2](./REMOTE_DESIGN.md).
