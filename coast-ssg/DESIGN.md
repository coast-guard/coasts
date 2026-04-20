# Shared Service Groups (SSG) — Design Document

> Status: Phase 0 (scaffolding + design) complete. Future phases will
> land against this document. Every section below is normative.

This document captures every decision made in the planning
conversation that produced the `coast-ssg` crate. Future agent sessions
should treat it as the source of truth after context compaction. The
companion [`README.md`](./README.md) is the discoverability shortcut;
this doc is where implementation details live.

## Ground rules (read before writing code)

These apply to every SSG PR without exception. Violations block merge.

1. **Use the make targets, not bare cargo.**
   - `make test` (runs `cargo test --workspace`) is the canonical test
     entrypoint.
   - `make lint` (runs `cargo fmt --all -- --check` **and**
     `cargo clippy --workspace -- -D warnings`) is the canonical lint
     entrypoint.
   - `make check` runs both together — use it before every commit.
   - See [`Makefile`](../Makefile) for the target definitions. If you
     need a narrower invocation while iterating (e.g.
     `cargo test -p coast-ssg`), that is fine, but the commit-gate
     check is always `make check`.
2. **Never suppress clippy issues.** This means:
   - No `#[allow(clippy::...)]`, `#[allow(dead_code)]`,
     `#[allow(unused_imports)]`, or equivalent attributes added to
     silence a lint. Fix the lint.
   - No `#[cfg_attr(..., allow(...))]` escape hatches.
   - No inline `#[expect(...)]` shortcuts on new code.
   - No `--allow` flags passed to clippy in CI or make targets.
   - If a lint genuinely cannot be fixed (extremely rare), the
     suppression MUST be accompanied by (a) a comment citing the
     upstream issue / PR number, and (b) a DESIGN.md §17 open-question
     entry so it is tracked and removed later.
3. **`make lint` clean is a phase exit criterion** (§21.4). Phases do
   not land on a branch where `make lint` reports any warning.
4. **Pre-existing lint failures elsewhere in the workspace are not
   SSG's problem to hide.** If the workspace already has clippy
   errors on the base branch, they are tracked separately. SSG code
   never adds new ones, but also never suppresses them to make
   `make lint` pass.
5. **Plan deviations MUST be reflected in this design doc.** If
   implementation diverges from the attached plan for any reason
   (crate boundaries, API shape, file location, dependency edges,
   etc.), the agent performing the implementation MUST update this
   DESIGN.md in the same PR to:
   - Describe the deviation in the relevant section so future code
     and agents see the current truth, not the stale plan intent.
   - Add a short "why we deviated" entry to §17 Open Questions
     (marked SETTLED) or §18 Risks as appropriate, including the
     original plan intent and the replacement decision.

   Plans are disposable once executed; this doc is the long-term
   source of truth. A deviation that is only documented in the plan
   file is not documented.

## Table of contents

- [§0 Implementation progress](#0-implementation-progress)
- [§1 Problem](#1-problem)
- [§2 Terminology](#2-terminology)
- [§3 Goals and non-goals](#3-goals-and-non-goals)
- [§4 High-level architecture](#4-high-level-architecture)
  - [§4.1 Crate layout](#41-crate-layout)
  - [§4.2 LLM-discoverability rules](#42-llm-discoverability-rules)
- [§5 SSG Coastfile format](#5-ssg-coastfile-format-coastfileshared_service_groups)
- [§6 Consumer Coastfile changes](#6-consumer-coastfile-changes)
  - [§6.1 SSG reference in coast build manifest](#61-coast-builds-record-an-ssg-reference)
- [§7 CLI surface](#7-cli-surface)
- [§8 Daemon state](#8-daemon-state)
- [§9 SSG lifecycle internals](#9-ssg-lifecycle-internals)
- [§10 Volumes / bind mounts](#10-volumes--bind-mounts)
- [§11 Port plumbing coast to SSG](#11-port-plumbing-coast--ssg)
- [§12 Host-side access + checkout](#12-host-side-access--coast-ssg-checkout)
- [§13 auto_create_db](#13-auto_create_db)
- [§14 inject semantics](#14-inject-semantics)
- [§15 File organization](#15-file-organization)
- [§16 Phased implementation plan](#16-phased-implementation-plan)
- [§17 Open questions](#17-open-questions)
- [§18 Risks](#18-risks)
- [§19 Success criteria](#19-success-criteria)
- [§20 Remote coasts with SSG](#20-remote-coasts-with-ssg)
- [§21 Development approach](#21-development-approach)
- [§22 Terminology cheat sheet](#22-terminology-cheat-sheet)

---

## 0. Implementation progress

Living checklist. Every PR that advances a phase MUST tick the boxes
it closes in the same commit.

Legend: `[ ]` not started, `[~]` in progress, `[x]` done.

### Phase 0 — Scaffolding and design doc
- [x] `coast-ssg` crate registered in the workspace
- [x] Module skeleton with `// TODO(ssg-phase-N)` placeholders
- [x] `README.md` (agent bootstrap)
- [x] `DESIGN.md` (this file) capturing every decision from the planning conversation
- [x] `cargo build -p coast-ssg` + `cargo clippy -p coast-ssg -- -D warnings` green (crate-scoped; `make lint` becomes the gate from Phase 1 onward per Ground Rules)

### Phase 1 — Data model and parser (no runtime)
- [x] `SsgCoastfile` type + validation in `coast-ssg/src/coastfile/`
- [x] Raw TOML structs in `coast-ssg/src/coastfile/raw_types.rs`
- [x] Consumer Coastfile extension: `[shared_services.<name>] from_group = true`
- [x] Conflict detection: same service name inlined and referenced
- [x] Forbidden-field checks when `from_group = true` (no `image`, `ports`, `env`, `volumes`)
- [x] `SsgRequest` / `SsgResponse` enum skeletons in `coast-core/src/protocol/ssg.rs`
- [x] Wire new variants into `coast-core::protocol::{Request, Response}`
- [x] Unit tests: parser happy paths + every error path
- [x] Unit tests: consumer Coastfile `from_group` acceptance + every forbidden-field error

### Phase 2 — SSG build
- [x] `coast ssg build` end to end (parse, pull images, cache tarballs, write artifact, flip `latest`, prune)
- [x] `coast ssg ps` reading artifact metadata only (no running container)
- [x] State DB migration: `ssg`, `ssg_services`, `ssg_port_checkouts` tables
- [x] `SsgStateExt` trait implemented on `coast-daemon::state::StateDb`
- [x] Integration test: `test_ssg_build_minimal`
- [x] Integration test: `test_ssg_build_multiple_services`
- [x] Integration test: `test_ssg_build_rebuild_prunes`
- [x] Unit tests: manifest round-trip, image cache path resolution

### Phase 3 — SSG run / stop / start / restart / rm
- [x] SSG singleton DinD creation via `coast-docker::DindRuntime`
- [x] Inner compose synthesis (`coast-ssg/src/runtime/compose_synth.rs`)
- [x] `docker compose up -d` inside DinD
- [x] Dynamic host-port allocation per inner service
- [x] Symmetric-path bind mounts (host → outer DinD → inner service)
- [x] `coast ssg logs` / `exec` / `ports` with optional `--service`
- [x] `ssg_mutex` on `AppState` guarding all mutating handlers
- [x] Integration test: `test_ssg_run_lifecycle`
- [x] Integration test: `test_ssg_bind_mount_symmetric`
- [x] Integration test: `test_ssg_named_volume_persists`
- [x] Unit tests: compose synth, bind-mount translation, port allocation

### Phase 3.5 — Auto-start hook in `coast run`
- [x] `coast-daemon/src/handlers/run/ssg_integration.rs` created
- [x] Auto-start SSG before provisioning a consumer coast that references it
- [x] Error "no SSG build exists" when no build is found
- [x] Progress events `SsgStarting` / `SsgStarted` emitted on the run stream
- [x] Integration test: `test_ssg_auto_start_on_run`

### Phase 4 — Coast ↔ SSG wiring
- [x] `ssg_integration::synthesize_shared_service_configs(cf, ssg_state)`
- [x] Consumer coasts' `from_group = true` services skip inline host-start
- [x] Existing `shared_service_routing` + `compose_rewrite` paths consume synthesized configs unchanged
- [x] Error: `from_group = true` references a name not in the active SSG
- [x] Integration test: `test_ssg_consumer_basic`
- [x] Integration test: `test_ssg_consumer_conflict`
- [x] Integration test: `test_ssg_consumer_missing_service`
- [x] Integration test: `test_ssg_port_collision` (two consumer coasts, one SSG postgres)

### Phase 4.5 — Remote coast + SSG
- [x] `rewrite_reverse_tunnel_pairs` in `coast-ssg/src/remote_tunnel.rs`
- [x] `setup_shared_service_tunnels` in `coast-daemon/src/handlers/run/mod.rs` consults SSG state
- [x] `coast ssg stop` / `rm` refuses while remote shadow instances reference it (unless `--force`)
- [x] Integration test: `test_ssg_remote_reverse_tunnel`
- [x] Integration test: `test_ssg_stop_blocked_by_remote`
- [x] Integration test: `test_ssg_stop_force_cleans_tunnels`

### Phase 5 — `auto_create_db` + `inject`
- [x] Nested exec (`coast-ssg/src/runtime/auto_create_db.rs`) reuses SQL from `coast-daemon/src/shared_services.rs::create_db_command`
- [x] `inject` resolution pulls the template from the SSG Coastfile, canonical inner port
- [x] Integration test: `test_ssg_auto_create_db`
- [x] Integration test: `test_ssg_inject_env`

### Phase 6 — Host-side canonical-port checkout
- [ ] `coast ssg checkout [service | --all]` and `uncheckout`
- [ ] `ssg_port_checkouts` writes + daemon-restart recovery
- [ ] Displacement of a coast-instance's canonical port with a clear warning
- [ ] `coast ssg ports` shows `(checked out)` annotation
- [ ] Integration test: `test_ssg_host_checkout`
- [ ] Integration test: `test_ssg_checkout_displaces_instance`

### Phase 7 — SSG reference in coast build manifest
- [ ] `coast build` embeds an `ssg` block in `manifest.json` when any `from_group = true` service exists
- [ ] `coast run` validates drift: match / same-image warn / missing-service error
- [ ] Integration test: `test_ssg_drift_warning`
- [ ] Integration test: `test_ssg_drift_missing_service`

### Phase 8 — Docs and polish
- [ ] `docs/concepts_and_terminology/SHARED_SERVICE_GROUPS.md`
- [ ] `docs/coastfiles/SHARED_SERVICE_GROUPS.md`
- [ ] `docs/concepts_and_terminology/SHARED_SERVICES.md` updated with "See also SSG"
- [ ] Host-volume migration recipe documented
- [ ] `coast ssg doctor` (warns on likely permission mismatches)
- [ ] `docs/coastfiles/README.md` reference table updated

---

## 1. Problem

Today, shared services are inlined in each project's `Coastfile` under
`[shared_services.*]`. They run as raw containers on the **host** Docker
daemon with canonical host ports (5432, 6379, ...). See
[`docs/concepts_and_terminology/SHARED_SERVICES.md`](../docs/concepts_and_terminology/SHARED_SERVICES.md).

Pain points:

1. **Host-port collisions across projects.** Two projects that both
   declare `postgres:5432` cannot run simultaneously.
2. **Host Docker Desktop sprawl.** Each project creates its own
   `{project}-shared-services` compose grouping.
3. **No cross-project sharing.** Even when two projects would be happy
   pointing at the same postgres, each one spins up its own.
4. **`auto_create_db` is project-bound.** Every project creates its DBs
   on its own postgres.

We want:

- Run shared services inside a singleton DinD on the host (the SSG).
- Give the SSG its own dynamic host ports. Canonical ports inside a
  coast still resolve to the right service via existing socat routing;
  the upstream is now an SSG dynamic port, not a canonical one.
- Let multiple projects reference the same SSG-owned service by name.
- Keep data shareable with the host via bind mounts from the host
  filesystem into SSG-managed services.

## 2. Terminology

- **SSG** — Shared Service Group. A singleton DinD container on the
  host that runs one or more shared services as nested containers.
- **SSG service** — one named shared service inside the SSG (e.g. `postgres`).
- **SSG Coastfile** — the top-level TOML file named
  `Coastfile.shared_service_groups`. Despite the plural, there is
  exactly one SSG per host at a time.
- **Canonical port** — the port an app talks to by name (`postgres:5432`).
  Unchanged from today's model.
- **SSG host port** — the dynamically allocated host port that the
  outer SSG DinD publishes on behalf of an SSG service. Replaces the
  canonical host port bindings used by inline shared services.
- **Consumer coast** — a regular coast that opts into an SSG-owned
  service via `[shared_services.<name>] from_group = true`.

## 3. Goals and non-goals

### Goals

- One SSG per host, managed via `coast ssg <verb>`.
- Typed TOML file `Coastfile.shared_service_groups` declaring services.
- Consumer Coastfile opt-in per service via `from_group = true`.
- Build-time validation that a service is never both inlined and SSG-referenced.
- Inside-coast DNS and port contract unchanged: services still
  resolve `postgres:5432` transparently.
- `auto_create_db` continues to work against SSG-owned postgres/mysql
  services (nested `docker exec`).
- **v1 is local-only.** Remote coasts may consume a local SSG via the
  existing reverse-SSH-tunnel mechanism (see §20).
- Inlined shared services continue to work unchanged. Migration is
  opt-in, per-service.

### Non-goals for v1

- Multiple concurrent SSGs on one host.
- A remote-resident SSG (i.e. an SSG running on a `coast-service` host).
- Automatic binding of SSG services to host canonical ports without
  user action. Users run `coast ssg checkout <service>` when they want
  host-side `localhost:5432`.
- Auto-migrating existing inlined shared services to SSG.
- `extends` / `includes` support in the SSG Coastfile (keep v1 simple).

## 4. High-level architecture

```text
Host Docker daemon
|
+-- coast-ssg (DinD, --privileged, singleton)          <-- NEW
|     +-- Inner Docker daemon
|     |     +-- postgres  (inner :5432)
|     |     +-- redis     (inner :6379)
|     |     +-- mongodb   (inner :27017)
|     +-- bind mounts: host dirs -> same paths inside DinD
|     +-- published ports: dynamic -> inner 5432/6379/27017
|
+-- coast: proj-a/dev-1  (existing DinD)
|     +-- docker0-alias socat forwarders
|           postgres:5432 -> host.docker.internal:{ssg-dyn-postgres}
|           redis:6379    -> host.docker.internal:{ssg-dyn-redis}
|
+-- coast: proj-b/dev-1
      +-- same story, same SSG, same host-side dynamic ports
```

Key insight: the existing `shared_service_routing` mechanism already
forwards `host.docker.internal:{host_port}` from inside each coast. All
we change is what the daemon puts into `host_port` when the coast
references an SSG service. The socat plumbing inside the coast is
unchanged.

### 4.1 Crate layout

```text
coast-ssg/                          <-- new library crate
  Cargo.toml
  README.md                         <-- agent bootstrap doc
  DESIGN.md                         <-- this file
  src/
    lib.rs                          <-- module list + crate-level doc
    coastfile/                      <-- Coastfile.shared_service_groups parser
      mod.rs
      raw_types.rs
    build/                          <-- artifact + image caching
      mod.rs
      artifact.rs
      images.rs
    runtime/                        <-- singleton DinD orchestration
      mod.rs
      lifecycle.rs
      compose_synth.rs
      bind_mounts.rs
      auto_create_db.rs
      ports.rs
    state.rs                        <-- SsgStateExt trait on StateDb
    paths.rs                        <-- ~/.coast/ssg/...
    port_checkout.rs                <-- host canonical-port checkout
    daemon_integration.rs           <-- public hooks coast-daemon calls
    remote_tunnel.rs                <-- reverse SSH tunnel pair helpers

coast-core/src/protocol/ssg.rs      <-- SsgRequest / SsgResponse (Phase 1)
coast-core/src/coastfile/...        <-- `from_group` field on consumer types

coast-daemon/src/handlers/ssg.rs               <-- request dispatcher (Phase 1+)
coast-daemon/src/handlers/run/ssg_integration.rs <-- pre-provision hook (Phase 3.5/4)
coast-cli/src/commands/ssg.rs                  <-- `coast ssg ...` (Phase 1+)

coast-service/                       <-- unchanged. Does NOT depend on coast-ssg.
```

**Dependency edges:**

- `coast-ssg` depends on `coast-core`, `coast-docker`, `coast-secrets`
  plus the usual workspace deps.
- `coast-daemon` depends on `coast-ssg` (from Phase 1 forward).
- `coast-cli` depends on `coast-core` only — it talks to the daemon
  over the existing Unix socket.
- `coast-service` does **not** depend on `coast-ssg`. This is a
  deliberate constraint: remote coasts reach the SSG via the
  pre-existing `SharedServicePortForward` protocol, and the SSG is
  local-only by construction (see §20).
- **Shared Docker helpers live in `coast-docker`, not `coast-core`.**
  `coast-core` intentionally has no `bollard` dependency (keeps types /
  protocol / coastfile parsing free of Docker's transitive dep graph).
  Any helper that both `coast-daemon` and `coast-ssg` need and that
  takes a `&bollard::Docker` goes into `coast-docker`. Concrete
  example: `coast_docker::image_cache::pull_and_cache_image` (Phase 2
  landed this lift from `coast-daemon/src/handlers/build/utils.rs`;
  the daemon now delegates to the `coast-docker` copy). If you find
  yourself wanting to add `bollard` to `coast-core`, stop and put the
  code in `coast-docker` instead.

### 4.2 LLM-discoverability rules

Normative. Reviewers must reject PRs that violate these.

**Principles**

1. One grep or one glob discovers the whole feature. `Glob **/*ssg*`
   from the repo root returns every SSG-related file. `rg '\bSsg'`
   returns every SSG type.
2. All public types are prefixed `Ssg`.
3. Feature logic lives in `coast-ssg`. Consumers call into it via
   `*ssg*`-named adapter files.
4. `coast-ssg/README.md` is the agent bootstrap. Reading it alone
   gives a full mental model of where everything lives.
5. A single index in the repo (the README "Where things live" table)
   lists every external touchpoint with full paths.

**Naming convention table**

| Concern                                | Convention                              |
|----------------------------------------|-----------------------------------------|
| Crate                                  | `coast-ssg`                             |
| Rust types                             | `Ssg*` prefix                           |
| Rust modules inside the feature crate  | lowercase descriptor (path already says `ssg`) |
| Files outside the feature crate        | must contain `ssg` in filename          |
| CLI verb                               | `coast ssg <subcommand>`                |
| Protocol enums                         | `SsgRequest::{Build, Run, ...}`         |
| DB tables                              | `ssg`, `ssg_services`, `ssg_port_checkouts` |
| Filesystem root                        | `~/.coast/ssg/`                         |
| User file name                         | `Coastfile.shared_service_groups`       |
| Consumer Coastfile field               | `[shared_services.<name>] from_group = true` |
| Docs filenames                         | `SHARED_SERVICE_GROUPS.md`              |
| Log targets                            | `target: "coast::ssg"`                  |
| Code-comment terminology               | "SSG" or "Shared Service Group". Never bare "group" or "singleton". |

**Banned patterns**

- Inlining `if has_ssg_ref { ... }` logic in a file whose name does
  not contain `ssg`. Factor into an adapter file (e.g.
  `coast-daemon/src/handlers/run/ssg_integration.rs`).
- Adding an `Option<Ssg...>` field to a cross-cutting type without a
  doc comment mentioning SSG.
- Any SSG-aware code in `coast-service`.
- The bare term "group" in SSG-specific identifiers or comments.

## 5. SSG Coastfile format (`Coastfile.shared_service_groups`)

Discovery mirrors the regular Coastfile build pipeline exactly (see
[`docs/concepts_and_terminology/BUILDS.md`](../docs/concepts_and_terminology/BUILDS.md)):

- `coast ssg build` in a directory with `Coastfile.shared_service_groups`
  (or `.toml` variant) uses that file. The usual tie-break rule
  applies.
- `coast ssg build -f <path>` points at an arbitrary file.
- `coast --working-dir <dir> ssg build` decouples the project root
  from the Coastfile location (matches `coast build --working-dir`).
- `coast ssg build --config '<inline-toml>'` supports scripting and CI
  flows.

The build artifact goes to `~/.coast/ssg/builds/{build_id}/` with a
`~/.coast/ssg/latest` symlink (see §9.1).

### Example

```toml
[ssg]
runtime = "dind"                  # optional; dind is the only supported runtime today
# [ssg.setup]
# packages = ["curl"]

[shared_services.postgres]
image = "postgres:16"
ports = [5432]                    # inner container port; host port is dynamic
volumes = [
    "/var/coast-data/postgres:/var/lib/postgresql/data",  # host bind mount
    "pg_wal:/var/lib/postgresql/wal",                      # inner named volume
]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["/var/coast-data/redis:/data"]

[shared_services.mongodb]
image = "mongo:7"
ports = [27017]
volumes = ["/var/coast-data/mongo:/data/db"]
env = { MONGO_INITDB_ROOT_USERNAME = "coast", MONGO_INITDB_ROOT_PASSWORD = "coast" }
```

### Rules

- Only `[ssg]` and `[shared_services.*]` sections are accepted. Any
  other top-level key (`[coast]`, `[ports]`, `[services]`, etc.) is
  rejected at parse.
- `extends` and `includes` are NOT supported in v1 (keep singleton
  simple). Track as §17 open question.
- `volumes` entries are one of two shapes (see §10):
  - `"/absolute/host/path:/container/path"` — host bind mount.
  - `"name:/container/path"` — inner named volume (lives inside the
    SSG's inner docker daemon; opaque to host).
- `ports` entries are bare container integers only. `"HOST:CONTAINER"`
  tuples are rejected — SSG host publications are always dynamic.
- `inject` is not allowed on SSG-service config. Inject happens on the
  consuming Coastfile, not here (see §14).
- `auto_create_db` is allowed and defaults to `false`.
- `env` is a flat string map, forwarded into the inner service container.

## 6. Consumer Coastfile changes

Consumers opt in **explicitly per service** using a flag on the
existing `[shared_services.*]` syntax. This keeps the mental model
("there's a shared service called postgres on this coast") while
making the migration from inline to SSG a one-line flip.

```toml
# Consumer Coastfile
[shared_services.postgres]
from_group = true

# Optional per-project overrides:
inject = "env:DATABASE_URL"   # env var name is project-local
# auto_create_db = true       # override (defaults to the SSG service's value)
```

### Rules when `from_group = true`

- `name` (the TOML key) must match a service in the active SSG build.
- `image`, `ports`, `env`, `volumes` are **forbidden**. The SSG is the
  single source of truth. Any of these fields present with
  `from_group = true` produces a parse-time error listing every
  forbidden field that was set.
- `inject` is **allowed**. Projects may expose the same SSG postgres
  under different env var names.
- `auto_create_db` is **allowed**. Overrides the SSG service's
  default. A consumer may explicitly disable per-instance DB creation
  for this project even if the SSG enables it.

### Conflict detection

At `coast build` and again at `coast run`:

- Two `[shared_services.<name>]` blocks with the same name in a single
  Coastfile are already rejected today. That stays.
- A block with `from_group = true` referencing a name that does not
  exist in the active SSG produces a clear error:
  `error: shared service 'postgres' references the shared service group, but no service 'postgres' exists in ~/.coast/ssg/latest.`
- A block with `from_group = true` plus any forbidden field produces:
  `error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports.`

Every SSG-referenced service must be declared explicitly. There is no
"auto-import all SSG services" shortcut.

### 6.1 Coast builds record an SSG reference

A regular `coast build` whose Coastfile contains at least one
`from_group = true` block records its dependency in `manifest.json`:

```json
{
  "ssg": {
    "build_id": "<SSG latest build_id at coast-build time>",
    "services": ["postgres", "redis"],
    "images": { "postgres": "postgres:16", "redis": "redis:7" }
  }
}
```

At `coast run`, drift handling:

1. `manifest.ssg.build_id` matches active SSG `latest` -> proceed.
2. Differs but image refs are identical for every referenced service
   -> warn and proceed.
3. Image refs differ or a referenced service is missing -> hard error:
   `"SSG has changed since this coast was built. Re-run `coast build` to pick up the new SSG, or pin the SSG to the old build."`

Pinning to an old SSG build (`coast ssg checkout-build <id>`) is
tracked as a future enhancement in §17; v1 requires rebuild.

## 7. CLI surface

Primary verb: `coast ssg`. Alias: `coast shared-service-group`.

| Command                                             | Purpose                                                        |
|-----------------------------------------------------|----------------------------------------------------------------|
| `coast ssg build [-f <file>] [--working-dir <dir>]` | Parse SSG Coastfile, pull images, write artifact, flip `latest`. |
| `coast ssg run`                                     | Create the singleton DinD and start all services. Allocates dynamic ports. |
| `coast ssg start`                                   | Start an existing but stopped SSG (services start with it).    |
| `coast ssg stop`                                    | Stop the SSG. Preserves state.                                 |
| `coast ssg restart`                                 | Stop + start.                                                  |
| `coast ssg rm [--with-data]`                        | Remove SSG container. `--with-data` also removes inner named volumes. |
| `coast ssg ps`                                      | Show SSG status + inner service statuses + dynamic host ports. |
| `coast ssg logs [--service <name>] [--tail N] [-f]` | Logs from the outer DinD or a specific inner service.          |
| `coast ssg exec [--service <name>] -- <cmd>`        | Exec into the SSG container or a specific inner service.       |
| `coast ssg ports`                                   | Per-service dynamic host ports (with `(checked out)` markers). |
| `coast ssg checkout [<service> \| --all]`           | Bind canonical host port via socat (§12).                      |
| `coast ssg uncheckout [<service> \| --all]`         | Tear down a checkout.                                          |

### Protocol sketch (`coast-core/src/protocol/ssg.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "action")]
pub enum SsgRequest {
    Build { file: Option<PathBuf>, working_dir: Option<PathBuf>, config: Option<String> },
    Run,
    Start,
    Stop,
    Restart,
    Rm { with_data: bool },
    Ps,
    Logs { service: Option<String>, tail: Option<u32>, follow: bool },
    Exec { service: Option<String>, command: Vec<String> },
    Ports,
    Checkout   { service: Option<String>, all: bool },
    Uncheckout { service: Option<String>, all: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SsgResponse {
    pub message: String,
    pub status: Option<String>,
    pub services: Vec<SsgServiceInfo>,
    pub ports: Vec<SsgPortInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SsgServiceInfo {
    pub name: String,
    pub image: String,
    pub inner_port: u16,
    pub dynamic_host_port: u16,
    pub container_id: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SsgPortInfo {
    pub service: String,
    pub canonical_port: u16,
    pub dynamic_host_port: u16,
    pub checked_out: bool,
}
```

Wired into `Request` / `Response` in `coast-core/src/protocol/mod.rs`.

## 8. Daemon state

New SQLite tables, added via migration in
[`coast-daemon/src/state/mod.rs`](../coast-daemon/src/state/mod.rs).
CRUD exposed as an `SsgStateExt` trait in `coast-ssg/src/state.rs`
(impl'd on `coast-daemon::state::StateDb` from within `coast-daemon`).

```sql
CREATE TABLE IF NOT EXISTS ssg (
    id              INTEGER PRIMARY KEY CHECK (id = 1),   -- singleton
    container_id    TEXT,
    status          TEXT NOT NULL,                        -- created / running / stopped
    build_id        TEXT,
    created_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ssg_services (
    service_name        TEXT PRIMARY KEY,
    container_port      INTEGER NOT NULL,
    dynamic_host_port   INTEGER NOT NULL,                 -- allocated on ssg run
    status              TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ssg_port_checkouts (
    canonical_port  INTEGER PRIMARY KEY,
    service_name    TEXT NOT NULL,
    socat_pid       INTEGER,
    created_at      TEXT NOT NULL
);
```

Notes:

- `CHECK (id = 1)` enforces the singleton.
- `ssg_services` is rebuilt every `coast ssg run` from the artifact.
- Dynamic port allocation reuses `coast_daemon::port_manager::allocate_dynamic_port`.
- `ssg_port_checkouts` is the mapping for §12.

## 9. SSG lifecycle internals

### 9.1 `coast ssg build`

1. Parse the SSG Coastfile via `coast-ssg/src/coastfile/`.
2. For each service: pull the image via
   `coast_docker::image_cache::pull_and_cache_image` into the shared
   `~/.coast/image-cache/` pool, record metadata in the manifest. The
   helper is shared with the regular `coast build` pipeline so the
   tarball naming convention is identical and cache hits from either
   side speed up the other (see §4.1).
3. Write `~/.coast/ssg/builds/{build_id}/`:
   - `manifest.json`
   - `ssg-coastfile.toml` (parsed + interpolated)
   - `compose.yml` (synthesized — see §9.2)
4. Flip `~/.coast/ssg/latest -> builds/{build_id}`.
5. Auto-prune older builds (reuse `coast-daemon/src/handlers/build/utils::auto_prune_builds`, keep 5).

### 9.2 Inner compose synthesis

The SSG runs inner services via `docker compose` inside DinD.
`coast-ssg/src/runtime/compose_synth.rs` generates:

```yaml
services:
  postgres:
    image: postgres:16
    environment: { POSTGRES_USER: coast, POSTGRES_PASSWORD: coast }
    ports: ["5432:5432"]                                       # inner-daemon publish
    volumes:
      - /var/coast-data/postgres:/var/lib/postgresql/data      # host bind (symmetric path, see §10)
      - pg_wal:/var/lib/postgresql/wal
    restart: unless-stopped
  redis:
    image: redis:7
    ports: ["6379:6379"]
    volumes:
      - /var/coast-data/redis:/data
    restart: unless-stopped

volumes:
  pg_wal: {}
```

The outer DinD separately publishes each service's inner port on a
dynamic host port — that layer is set up by [`crate::runtime::lifecycle`].

### 9.3 `coast ssg run`

Pseudocode:

```rust
let build = paths::resolve_latest_ssg_build()?;
let planned_ports = ports::allocate_ssg_service_ports(&build.services)?;

let mut dind_cfg = DindConfigParams::new("coast", "ssg", &build.artifact_dir);
dind_cfg.bind_mounts.extend(bind_mounts::outer_bind_mounts(&build));
dind_cfg.volume_mounts.push(ssg_docker_state_volume());        // /var/lib/docker
for (svc, p) in &planned_ports {
    dind_cfg.published_ports.push(PortPublish {
        host_port: p.dynamic,
        container_port: p.inner,
    });
}

let cid = runtime.create_coast_container(&dind_cfg).await?;
runtime.start_coast_container(&cid).await?;
wait_for_inner_daemon(&cid).await?;
load_cached_images_into_inner(&cid, &build).await?;
exec_inner(&cid, "docker compose -f /coast-artifact/compose.yml up -d").await?;

state.ssg.write_running(cid, build.id);
state.ssg_services.upsert_all(&planned_ports);
```

### 9.4 `coast ssg stop` / `start` / `restart` / `rm`

Each verb operates on the one SSG container only. `rm` preserves data
by default; `--with-data` removes inner named volumes before the SSG
container is removed. See §20.6 for the remote-coast safety check on
`stop` and `rm`.

## 10. Volumes / bind mounts

### 10.1 Declaration shapes

```toml
[shared_services.postgres]
volumes = [
    "/var/coast-data/postgres:/var/lib/postgresql/data",   # A: host bind mount
    "pg_wal:/var/lib/postgresql/wal",                       # B: inner named volume
]
```

- **A — host bind mount.** Source starts with `/`. Bytes live on the
  user's actual host filesystem. Host agents, `ls`, backups all see
  the same bytes the inner postgres sees.
- **B — inner named volume.** Source is a Docker volume name (no `/`).
  Volume lives inside the SSG DinD's inner docker daemon. Persists
  across SSG restarts (inner `/var/lib/docker` is itself a named host
  volume), but is opaque to the host.

Rejected at parse:

- Relative paths (`./data:/...`).
- Container-only volumes (no source).
- `..` components.
- Duplicate targets within one service.

### 10.2 The symmetric-path plan

The user writes `"/var/coast-data/postgres:/var/lib/postgresql/data"`.
At `coast ssg run`, the **same host path string** is used in both
mount hops:

**Hop 1 — outer DinD container creation**

```text
bind: /var/coast-data/postgres -> /var/coast-data/postgres
```

After this hop, `/var/coast-data/postgres` exists inside the DinD
container's filesystem and reads/writes pass through to the host.

**Hop 2 — inner compose (synthesized by `compose_synth.rs`)**

```yaml
volumes:
  - /var/coast-data/postgres:/var/lib/postgresql/data
```

The inner docker daemon runs inside the DinD container with
`--privileged`. Its bind-mount resolution happens in its own
filesystem view, which IS the DinD container's filesystem, which IS
the host directory (via Hop 1). Same inode, same bytes, three names
for one thing.

```text
+-- Host filesystem ------------------------------+
| /var/coast-data/postgres/         (real dir)    |
| |-- base/  PG_VERSION  ...                      |
+-------+-----------------------------------------+
        | Hop 1: -v /var/coast-data/postgres:/var/coast-data/postgres
        v
+-- SSG DinD container (coast-ssg) ---------------+
| /var/coast-data/postgres/         (same inodes) |
| /var/lib/docker/                  (named vol)   |
| Inner dockerd runs here.                        |
+-------+-----------------------------------------+
        | Hop 2: - /var/coast-data/postgres:/var/lib/postgresql/data
        v
+-- Inner postgres container --------------------+
| /var/lib/postgresql/data/         (same inodes)|
+------------------------------------------------+
```

**Why symmetric paths** (vs. remapping to `/coast-ssg-vols/{svc}/{i}`):

1. Log legibility: postgres errors that cite
   `/var/lib/postgresql/data/base/1/...` are traceable by
   `ls /var/coast-data/postgres/base/1/...` on the host with no
   mental translation.
2. Error messages echo user intent.
3. No synth-side naming scheme to maintain.
4. Grep-friendly — the user's path appears verbatim in their Coastfile
   and everywhere else.

### 10.3 Validation and runtime behavior

Implemented in `coast-ssg/src/runtime/bind_mounts.rs`:

1. **Parse-time** — rules in §10.1.
2. **Pre-run** — for every host bind source, `mkdir -p` with the
   calling user's UID/GID before creating the DinD container.
3. **Outer DinD** — each host bind becomes a `bollard::models::Mount`
   with `BIND` type and propagation `rprivate` (default).
4. **Inner compose** — source path is passed verbatim.

### 10.4 Lifecycle

- `coast ssg rm` never touches host bind mount contents.
- Inner named volumes survive `rm` by default.
- `coast ssg rm --with-data` runs `docker volume rm` on each inner
  named volume (inside the SSG DinD) before removing the DinD itself.

### 10.5 Permissions caveat

Some images (postgres UID 999, etc.) require pre-set ownership on
their data directories. v1 documents this in
`docs/coastfiles/SHARED_SERVICE_GROUPS.md` with a `chown 999:999`
example. v2 considers a per-volume `chown = "999:999"` /
`mode = "0700"` field and / or a `coast ssg doctor` subcommand that
warns on likely mismatches for known images.

### 10.6 Platform notes

- **macOS Docker Desktop** — host paths must be in Settings →
  Resources → File Sharing. Defaults include `/Users`, `/Volumes`,
  `/private`, `/tmp`. `/var/coast-data` is **not** in the default
  list on macOS; user docs should prefer `$HOME/coast-data/...`.
- **WSL2** — prefer WSL-native paths (`~`, `/mnt/wsl/...`).
  `/mnt/c/...` works but is slow.
- **Linux** — no gotchas.

### 10.7 Migrating from an existing host Docker named volume

Users with data inside a host Docker named volume
(`infra_postgres_data:/var/lib/postgresql/data`) can migrate without
copying bytes by binding the volume's mountpoint directly:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
```

Ugly but zero-copy. A future `coast ssg import-host-volume <name>`
subcommand can automate this — out of scope for v1.

## 11. Port plumbing (coast -> SSG)

Consumer coasts still believe their services are at canonical ports
(`postgres:5432`). Flow at `coast run`:

1. Daemon resolves the consumer's `from_group = true` services.
2. For each, looks up `(container_port, dynamic_host_port)` in
   `ssg_services`.
3. `ssg_integration::synthesize_shared_service_configs` builds a
   synthetic `SharedServiceConfig` per service with
   `ports = [SharedServicePort { host_port: dynamic, container_port: canonical }]`.
4. The existing pipeline consumes those unchanged:
   - [`coast-daemon/src/handlers/shared_service_routing.rs`](../coast-daemon/src/handlers/shared_service_routing.rs)
     computes docker0 alias IPs and spawns socat forwarders inside the
     coast's DinD: `TCP-LISTEN:{canonical},bind={alias_ip}` -> `TCP:host.docker.internal:{dynamic}`.
   - [`coast-daemon/src/handlers/run/compose_rewrite.rs`](../coast-daemon/src/handlers/run/compose_rewrite.rs)
     adds `extra_hosts: {name}: {alias_ip}` into the compose.
5. The existing inline-start path
   ([`run/shared_services_setup.rs`](../coast-daemon/src/handlers/run/shared_services_setup.rs))
   is skipped for `from_group = true` services — the SSG is already
   running, nothing to start on the host daemon.

Net effect: app code and compose DNS are completely unchanged. The
only thing different is what lives on the other side of
`host.docker.internal:{host_port}` — the SSG DinD's published port
instead of a host-daemon postgres.

### 11.1 Auto-start semantics

`coast run` auto-starts the SSG when a consumer coast needs it and it
is not currently running.

- SSG build exists, container not running -> daemon runs the
  equivalent of `coast ssg start` (or `run` if the container was never
  created), guarded by the process-global `ssg_mutex` from §17-5.
- No SSG build exists at all -> hard error:
  `"Project 'X' references shared service 'postgres' from the Shared Service Group, but no SSG build exists. Run coast ssg build in the directory containing your Coastfile.shared_service_groups."`
- Emit `CoastEvent::SsgStarting` / `SsgStarted` on the run progress
  channel so Coastguard can show boot progress inline.

## 12. Host-side access + `coast ssg checkout`

Coast-internal traffic always goes through the docker0 alias-IP socat
path. **Host-side** callers (MCPs, ad-hoc `psql`, Coastguard previews)
use the new checkout command:

```bash
coast ssg checkout postgres
coast ssg checkout --all
coast ssg uncheckout postgres
```

Semantics:

- `checkout <service>` spawns a host-level socat listening on the
  canonical port (5432 for postgres) and forwarding to the SSG's
  dynamic host port. Implemented via `coast-daemon::port_manager::PortForwarder`.
- Only one owner per canonical port. If a coast instance is currently
  checked out on canonical 5432 (because it had an inline postgres or
  a same-canonical-port app server), the SSG checkout displaces it:
  1. Look up the current holder in `port_allocations`.
  2. Kill the existing socat PID.
  3. Clear `port_allocations.is_primary = 1` for that holder.
  4. Spawn the SSG-owned socat.
  5. Record the SSG as the new owner in `ssg_port_checkouts`.
- Displacement is visible: CLI prints a warning listing what was
  displaced; Coastguard emits a structured event. No `--force` flag
  is required, but the message is unambiguous.
- The displaced coast instance is NOT auto-rebound when the SSG
  uncheckouts. It stays on dynamic-only until the user runs
  `coast checkout <instance>` again.
- Coasts **always** reach SSG services via the docker0 alias-IP
  path, regardless of host-side checkout state. Checkout is purely a
  host-side convenience.

## 13. `auto_create_db`

Phase 5 lights up `auto_create_db` for both paths (inline and SSG)
— prior to that, the SQL builder existed but had no caller (§17-20).
Inline services run `docker exec <host-container> psql ... \gexec`
against the shared-services host container. SSG services run a
nested exec (`coast-ssg/src/runtime/auto_create_db.rs`):

```text
docker exec <ssg-outer> \
  docker exec <inner-postgres-container> \
  psql -U postgres -c "... \\gexec"
```

The SQL command construction is shared with the inline path
([`coast-daemon/src/shared_services.rs::create_db_command`](../coast-daemon/src/shared_services.rs)).
The nested-exec wrapper lives in `coast-ssg` and is exposed to the
daemon via `daemon_integration::create_instance_db_for_consumer`.

This keeps auto_create_db fully local-side even for remote consumer
coasts — see §20.4.

## 14. `inject` semantics

A consumer coast's `[shared_services.<name>].inject` resolves against
the referenced SSG service's metadata:

- `${host}` is substituted with the DNS name the coast uses (the
  service name, e.g. `postgres`) — NOT the dynamic host port.
- `${port}` is the canonical inner port (5432) — NOT the dynamic host
  port.
- Any per-project overrides (e.g. username / password / DB name) come
  from the consumer Coastfile's `inject` template or from secrets
  declared in the consumer Coastfile.

Result: the inject string a coast sees at runtime is always something
like `postgres://coast:coast@postgres:5432/app`, regardless of the
SSG's dynamic port. That invariance is the whole point.

Phase 5 scope (`inject`): `env:NAME` is fully wired end-to-end for
both inline and SSG shared services via
[`coast-daemon/src/shared_services.rs::shared_service_inject_env_vars`].
`file:/path` is recognized by the parser but is a deferred follow-up;
the runtime currently skips file-inject silently. See §17-21 and the
issue tracker once this doc is split.

## 15. File organization

Every SSG-touching file in the repository is captured in the
`README.md` "Where things live" table. The adapter-file pattern in
`coast-daemon` ensures agents can follow a single call from
`provision.rs` into one `*ssg*`-named adapter into the feature crate.

Touchpoint counts by crate (after all phases land):

- `coast-ssg/` — everything self-contained (library, README, DESIGN).
- `coast-core/` — 2 additions: `protocol/ssg.rs`, `from_group` field.
- `coast-daemon/` — exactly 2 `*ssg*` files (handlers/ssg.rs,
  handlers/run/ssg_integration.rs) plus single-line call sites in
  `provision.rs` and `handle_remote_run`.
- `coast-cli/` — 1 file (`commands/ssg.rs`).
- `coast-service/` — zero files. Enforced by absence of dependency.
- `docs/` — 2 new pages plus a "see also" paragraph in the existing
  shared-services doc.

## 16. Phased implementation plan

Each phase ends at a commit-able state with tests green. See §21 for
normative rules on how each phase is built (design-first, integration
tests via dindind, unit tests everywhere). The progress tracker in §0
tracks state across sessions.

### Phase 0 — Scaffolding + design doc (DONE, this commit)

- `coast-ssg` crate, module skeleton, README, DESIGN.

### Phase 1 — Data model and parser (no runtime)

- `SsgCoastfile` parser in `coast-ssg`.
- Consumer-side `from_group` field in `coast-core::coastfile`.
- Conflict + forbidden-field validation.
- `SsgRequest` / `SsgResponse` skeletons in `coast-core`.
- Unit tests for every accept / reject path.
- No daemon wiring yet.

### Phase 2 — SSG build

- `coast ssg build` end to end.
- State DB migrations for `ssg`, `ssg_services`, `ssg_port_checkouts`.
- `coast ssg ps` reading artifact metadata.
- Integration tests: `test_ssg_build_minimal`, `test_ssg_build_multiple_services`, `test_ssg_build_rebuild_prunes`.

### Phase 3 — SSG run / stop / start / rm

- Singleton DinD creation.
- Inner compose synthesis + `compose up -d`.
- Dynamic port allocation + publication.
- Symmetric-path bind mounts.
- `coast ssg logs` / `exec` / `ports`.
- Integration tests: `test_ssg_run_lifecycle`, `test_ssg_bind_mount_symmetric`, `test_ssg_named_volume_persists`.

### Phase 3.5 — Auto-start hook in `coast run`

- `handlers/run/ssg_integration.rs::ensure_ready_for_instance`.
- Progress event wiring.
- Integration test: `test_ssg_auto_start_on_run`.

### Phase 4 — Coast ↔ SSG wiring

- `synthesize_shared_service_configs` from SSG state.
- Skip inline-start for `from_group = true` services.
- Integration tests: `test_ssg_consumer_basic`, `test_ssg_consumer_conflict`, `test_ssg_consumer_missing_service`, `test_ssg_port_collision`.

### Phase 4.5 — Remote coast + SSG

- `rewrite_reverse_tunnel_pairs` (§20.2).
- Auto-start ordering in `handle_remote_run`.
- `coast ssg stop` / `rm` refuses while remote shadow instances use it.
- Integration tests: `test_ssg_remote_reverse_tunnel`, `test_ssg_stop_blocked_by_remote`, `test_ssg_stop_force_cleans_tunnels`.

### Phase 5 — `auto_create_db` and `inject`

- Nested exec for postgres / mysql.
- `inject` resolution against SSG Coastfile.
- Integration tests: `test_ssg_auto_create_db`, `test_ssg_inject_env`.

### Phase 6 — Host-side canonical-port checkout

- `coast ssg checkout` / `uncheckout`.
- Displacement of coast-instance canonical holders.
- Daemon-restart recovery of active checkouts.
- Integration tests: `test_ssg_host_checkout`, `test_ssg_checkout_displaces_instance`.

### Phase 7 — SSG reference in coast build manifest

- `coast build` embeds `ssg` block.
- `coast run` drift detection (match / same-image warn / missing error).
- Integration tests: `test_ssg_drift_warning`, `test_ssg_drift_missing_service`.

### Phase 8 — Docs and polish

- `docs/concepts_and_terminology/SHARED_SERVICE_GROUPS.md`.
- `docs/coastfiles/SHARED_SERVICE_GROUPS.md`.
- `docs/concepts_and_terminology/SHARED_SERVICES.md` "see also" update.
- Host-volume migration recipe.
- Optional: `coast ssg doctor`.

## 17. Open questions

1. (SETTLED) Primary CLI verb is `coast ssg`, alias `coast shared-service-group`.
2. (SETTLED) File discovery mirrors `coast build` (cwd lookup, `-f`, `--working-dir`, inline `--config`).
3. **Per-project network** — today `coast-shared-{project}` networks
   are created and coasts are attached. Since SSG routing uses
   `host.docker.internal` via docker0 alias IPs, the network is not
   strictly required. Proposal: keep the network creation (it is
   cheap, keeps the compose rewriter uniform, and lets users attach
   ad-hoc containers). Revisit if it causes issues.
4. (SETTLED) Auto-start SSG from `coast run` (hard error only if no
   SSG build exists).
5. **Concurrent mutation** — every SSG-mutating handler acquires a
   process-global `ssg_mutex: tokio::sync::Mutex<()>` in `AppState`.
   Coast-run paths that auto-start the SSG check state first, then
   acquire the write lock only when actually starting. TBD whether an
   `RwLock` is warranted instead — decide in Phase 3.5.
6. (SETTLED) Remote coasts use the existing
   `SharedServicePortForward` protocol (§20). `coast-service` is
   unchanged. Remote SSG is an explicit non-goal for v1.
7. **Coastfile.shared_service_groups inheritance** — v1 disallows
   `extends` / `includes`. Reconsider in v2 once real-world usage
   shows whether users need composition (e.g. "dev-ssg extends
   base-ssg plus a test-seed service").
8. **Checkout displacement UX** — proposal: no `--force` flag, emit a
   clear CLI warning and a Coastguard event. Confirm with a real use
   case in Phase 6.
9. **Pinning a consumer coast to an older SSG build** — `coast ssg
   checkout-build <id>` mentioned in §6.1 as a future convenience. Not
   in v1; v1 requires `coast build` to pick up SSG changes.
10. (SETTLED — Phase 2) **Home for shared Docker helpers.** Original
    plan wanted to lift `pull_and_cache_image` from `coast-daemon` into
    `coast-core` so `coast-ssg` could reuse it. This was changed to
    lift into `coast-docker` instead because `coast-core` has no
    `bollard` dependency and taking one would propagate Docker's
    transitive deps (~40 crates) into every consumer of
    `coast-core` — including `coast-cli`, which talks to the daemon
    over a socket and should not pull Docker. `coast-docker` already
    owns Docker primitives and was the topologically correct home.
    Future shared Docker helpers follow this same rule (§4.1).
11. (SETTLED — Phase 3) **SSG singleton container naming.** DESIGN.md
    §4 specifies the singleton is called `coast-ssg`, but the existing
    `coast-docker` `ContainerConfig` always produces
    `{project}-coasts-{instance}` (e.g. `coast-coasts-ssg`). Rather
    than accept that awkward name, Phase 3 added
    `ContainerConfig.container_name_override: Option<String>` (and
    the matching `DindConfigParams.container_name_override`) so the
    SSG lifecycle can request the literal name verbatim. All other
    callers leave the field `None` and continue to use the default
    convention. See
    [`coast-docker/src/runtime.rs`](../coast-docker/src/runtime.rs)
    and
    [`coast-docker/src/dind.rs`](../coast-docker/src/dind.rs).
12. (SETTLED — Phase 3) **Pure port-allocation helpers lifted to
    `coast-core`.** `coast-ssg/src/runtime/ports.rs` needs
    `allocate_dynamic_port_excluding`, but pulling it from
    `coast-daemon` would create a cycle
    (`coast-ssg -> coast-daemon -> coast-ssg`). The pure TCP-bind
    probe functions (`allocate_dynamic_port`,
    `allocate_dynamic_port_excluding`, `is_port_available`,
    `inspect_port_binding`, `PortBindStatus`) moved into the new
    [`coast-core/src/port.rs`](../coast-core/src/port.rs); the
    daemon's `port_manager` keeps its socat/checkout orchestration and
    delegates to `coast_core::port::*` for the allocation primitives.
    This mirrors the Phase 2 `pull_and_cache_image` lift pattern
    (§17.10) but targets `coast-core` because the helpers have no
    Docker dependency (they only touch `std::net::TcpListener`).
13. (SETTLED — Phase 3) **Lifecycle functions do not hold
    `&dyn SsgStateExt` across `.await`.** The daemon's `StateDb`
    wraps a `rusqlite::Connection` which is `!Sync`. Passing
    `&dyn SsgStateExt` into a lifecycle function that awaits Docker
    work would reject the `Send` bound on the resulting streaming
    future. Lifecycle orchestrators therefore take plain input
    records (`SsgRecord`, `Vec<SsgServicePortPlan>`) and return
    outcome types (`SsgRunOutcome`, `SsgStartOutcome`,
    `SsgStopOutcome`). Daemon handlers read state before the async
    section and apply writes afterwards
    (`apply_to_state_and_response`). `ports_ssg` is the one
    exception — it is synchronous and does no Docker work, so it
    takes `&dyn SsgStateExt` directly. See
    [`coast-ssg/src/runtime/lifecycle.rs`](./src/runtime/lifecycle.rs)
    and
    [`coast-daemon/src/handlers/ssg.rs`](../coast-daemon/src/handlers/ssg.rs).
14. (SETTLED — Phase 3) **`Response::SsgLogChunk` is a struct variant,
    not a tuple newtype.** The `Response` enum uses
    `#[serde(tag = "type")]` (internally tagged) which cannot
    serialize tuple variants holding a primitive string. The chunk
    payload therefore lives in a dedicated
    `SsgLogChunk { chunk: String }` struct, tagged into the enum as
    `Response::SsgLogChunk(SsgLogChunk)`. This is purely a
    serialization workaround; the wire format still carries a single
    string payload per chunk.
15. (SETTLED — Phase 3) **Shared `with_coast_home` helper in
    `coast-ssg`.** Each test module originally kept its own
    `ENV_LOCK: Mutex<()>` for serializing `COAST_HOME` overrides,
    but those per-module mutexes don't protect across modules — a
    test in `paths::tests` could race with a test in
    `build::artifact::tests`. Phase 3 consolidates the lock into
    [`coast-ssg/src/test_support.rs`](./src/test_support.rs) so every
    test that mutates `COAST_HOME` acquires the same mutex. Exposes
    one public helper, `with_coast_home(|root| ...)`, used by both
    `paths::tests` and `build::artifact::tests`.
16. (SETTLED — Phase 3.5) **`CoastEvent::SsgStarting` /
    `SsgStarted` payload shape.** §11.1 mentioned the variant names
    but didn't specify fields. Phase 3.5 landed
    `{ project: String, build_id: String }` for both, where
    `project` is the *consumer* coast that triggered the auto-start
    (for UX attribution — Coastguard can surface "`my-app` is
    starting the SSG") and `build_id` is the SSG build about to be
    brought up. Events emitted unconditionally by every successful
    auto-start, including the `already running` short-circuit, so
    subscribers can rely on the pair as a standard handshake. See
    [`coast-core/src/protocol/events.rs`](../coast-core/src/protocol/events.rs)
    and
    [`coast-daemon/src/handlers/run/ssg_integration.rs`](../coast-daemon/src/handlers/run/ssg_integration.rs).
17. (SETTLED — Phase 3.5) **`Ensure SSG ready` progress step uses a
    fixed 1-of-1 plan and prefixes nested events.** `BuildProgressEvent`
    has one `(step_number, total_steps)` per event, and the consumer
    `coast run` progress plan is fixed before provisioning starts;
    we can't retroactively extend it with 3-6 additional sub-steps
    from inside `run_ssg` / `start_ssg`. Phase 3.5 therefore emits a
    single outer `Ensure SSG ready` step (`started` + `done`) and
    forwards the inner `run_ssg` / `start_ssg` events with a
    `SSG: ` prefix on their `step` field so the CLI shows the full
    boot sequence without breaking the consumer's progress plan.
    Idempotent — re-prefixing is a no-op, so nested calls stay flat.
21. (SETTLED — Phase 5) **DB naming convention for `auto_create_db`
    is `{instance}_{project}`.** The inline
    [`coast_docker::compose::build_connection_url`](../coast-docker/src/compose.rs)
    already encodes `{instance}_{project}` into the connection string
    it emits. Using the same shape for
    [`consumer_db_name`](../coast-daemon/src/shared_services.rs) means
    the DB that `auto_create_db` creates and the DB that `inject`
    points at are guaranteed to agree — no wiring required.
    Alternatives considered: `{instance}` alone (the
    `database_name(instance, "")` shape from the v1
    `auto_create_db_names` helper), or `{instance}_{POSTGRES_DB}`
    (reading the base name from the service's own env). Both would
    decouple the DB name from the inject URL and introduce new
    failure modes.
20. (SETTLED — Phase 5) **Inline `auto_create_db` was not actually
    implemented before Phase 5, despite §13 claiming otherwise.**
    [`coast-daemon/src/shared_services.rs::create_db_command`](../coast-daemon/src/shared_services.rs)
    has existed since before Phase 0 but had no caller. Similarly,
    [`coast-docker/src/compose.rs::generate_shared_service_override`](../coast-docker/src/compose.rs)
    writes the inject connection URL as a YAML comment rather than as
    an actual `environment:` entry — so inject was never realized in
    container env either. Phase 5 adds the runtime callers for both
    paths (inline: direct `docker exec`; SSG: nested compose-exec)
    via
    [`coast-daemon/src/handlers/run/auto_create_db.rs::run_auto_create_dbs`](../coast-daemon/src/handlers/run/auto_create_db.rs)
    and
    [`coast-daemon/src/shared_services.rs::shared_service_inject_env_vars`](../coast-daemon/src/shared_services.rs).
    The SQL builder and connection-URL builder are reused verbatim
    so inline and SSG paths emit byte-identical DDL + env vars.
    `file:/path` inject is deferred — parsed but runtime skips it.
    Integration coverage: `test_ssg_auto_create_db`,
    `test_ssg_inject_env`, `test_shared_service_auto_create_db`.
19. (SETTLED — Phase 4.5) **`shared_service_tunnel_pids` is an
    in-memory-only map, not a SQLite table.** Phase 4.5 needs
    `coast ssg stop/rm --force` to tear down reverse SSH tunnels for
    remote shadow coasts that currently consume the SSG. We track
    those child PIDs in
    `AppState.shared_service_tunnel_pids: Mutex<HashMap<(String, String),
    Vec<u32>>>` keyed by `(project, instance_name)`. Reverse tunnels
    are per-run child processes: if the daemon restarts, the PIDs
    are gone anyway, and
    [`restore_tunnels_for_instance`](../coast-daemon/src/lib.rs)
    re-spawns fresh ones that repopulate this map. Persisting to
    SQLite would buy nothing (stale PIDs become meaningless after
    any process exit) and would add churn on every tunnel spawn.
    Populated in three places:
    `handlers/run/mod.rs::setup_shared_service_tunnels` (normal run),
    `lib.rs::create_reverse_tunnels` (daemon-restart restore), and
    `handlers/start.rs::reestablish_shared_service_tunnels` (coast start).
    Consumed only by `handlers/ssg.rs::handle_stop/handle_rm` when
    `--force` is set.
18. (SETTLED — Phase 4) **`shared_service_targets` placeholder for
    SSG-backed services is the literal string `"coast-ssg"`.**
    [`coast-daemon/src/handlers/shared_service_routing.rs`](../coast-daemon/src/handlers/shared_service_routing.rs)
    uses the `target_containers: HashMap<String, String>` map only
    for a `.contains_key(service.name)` existence check; the actual
    socat upstream is always `host.docker.internal:<host_port>` via
    the `SOCAT_UPSTREAM_HOST` constant. Rather than invent a new
    per-service value that implies a nonexistent on-host container,
    Phase 4 inserts the literal `"coast-ssg"` for every synthesized
    SSG service. The value is intentionally self-documenting: a
    future reader searching the state dump sees exactly where these
    entries come from. Consumers of `target_containers` must treat
    a value of `"coast-ssg"` as "routed through the SSG singleton,
    not an on-host inline container".

## 18. Risks

- **Hidden port collisions.** Dynamic ports can clash with other
  processes that grab the port between allocation and `docker run`.
  Mitigated by the existing `allocate_dynamic_port` helper which
  already handles retries; SSG reuses it.
- **DinD-in-DinD confusion.** Coasts are DinD; the SSG is DinD. They
  are *siblings* on the host Docker daemon, not nested. Coasts reach
  the SSG via `host.docker.internal`, never by nesting. Add a
  paragraph to user docs so this is clear.
- **Docker Desktop UX regression.** Users lose the per-project compose
  view for databases. Mitigation: label the SSG's inner compose
  project as `coast-ssg` consistently, and document that inner
  services are visible via `coast ssg ps`.
- **Volume migration friction.** The biggest footgun for adopters.
  Mitigate with a docs recipe (§10.7), `coast ssg doctor` warnings,
  and a future `coast ssg import-host-volume`.
- **State drift.** The SSG's `ssg_services` table and the live DinD
  could disagree (e.g. after manual `docker stop`). `coast ssg ps`
  must inspect live Docker state and reconcile, like
  [`coast-daemon/src/handlers/shared.rs::fetch_shared_services`](../coast-daemon/src/handlers/shared.rs).

## 19. Success criteria

- A user can declare postgres + redis once in
  `Coastfile.shared_service_groups`, reference them from three different
  projects' Coastfiles via `[shared_services.<name>] from_group =
  true`, run all three coasts concurrently, and have each app
  container talk to `postgres:5432` / `redis:6379` as normal with zero
  host-port conflicts.
- `coast ssg ps` shows the SSG and all services with images, dynamic
  host ports, statuses.
- Building a Coastfile that both inlines and SSG-references the same
  service name fails fast with a clear message.
- A running consumer coast that pointed at an inline postgres can be
  switched to an SSG postgres by flipping two lines in its Coastfile
  and re-building — no application code changes.
- Existing Coastfiles using only inline `[shared_services.*]` keep
  working unchanged (zero-migration guarantee).

## 20. Remote coasts with SSG

### 20.1 The contract

`coast-service` never learns about SSGs. It already speaks a
daemon-agnostic contract via `RunRequest::shared_service_ports:
Vec<SharedServicePortForward>`:

```rust
pub struct SharedServicePortForward { pub name: String, pub port: u16 }
```

On the remote side, [`coast-service/src/handlers/run.rs`](../coast-service/src/handlers/run.rs):

1. Reads `req.shared_service_ports`.
2. Strips the named services from the inner compose project.
3. Adds `extra_hosts: {name}: host-gateway` so inner containers
   resolve the service DNS name to `host.docker.internal` inside the
   remote DinD.
4. Adds `host.docker.internal:host-gateway` to the remote DinD's own
   extra hosts.

From the remote's point of view, there is "some process" on the other
side of the reverse SSH tunnel on port `{container_port}`. What that
process is — inline shared service, SSG-owned service, or future
remote-SSG — is invisible.

### 20.2 Local-side changes only

In `coast-daemon/src/handlers/run/mod.rs::setup_shared_service_tunnels`,
only the **local** side of each reverse-tunnel pair changes:

```rust
// Existing:
let reverse_pairs: Vec<(u16, u16)> =
    forwards.iter().map(|fwd| (fwd.port, fwd.port)).collect();

// After Phase 4.5:
let reverse_pairs = coast_ssg::remote_tunnel::rewrite_reverse_tunnel_pairs(
    forwards,
    &ssg_state,
);
// For each fwd:
//   (fwd.port /* remote canonical */, ssg_services[fwd.name].dynamic_host_port /* local */)
//   falls back to (fwd.port, fwd.port) when the service is inline.
```

The tunnel itself is built by the existing
[`coast-daemon/src/handlers/remote/tunnel.rs::reverse_forward_ports`](../coast-daemon/src/handlers/remote/tunnel.rs),
which already accepts arbitrary `(remote_port, local_port)` pairs:

```text
ssh -R 0.0.0.0:{remote_port}:localhost:{local_port} ...
```

No signature change, no new ssh flag, no new RPC.

### 20.3 Flow diagram

```text
REMOTE (coast-service)                       LOCAL (coast-daemon + coast-ssg)
+----------------------------+               +-----------------------------+
| inner app container        |               |                             |
|   -> postgres:5432         |               |                             |
|   (extra_hosts override)   |               |                             |
|       |                    |               |                             |
|       v                    |               |                             |
| DinD host-gateway          |               |                             |
|       |                    |               |                             |
|       v                    |               |                             |
| DinD published port 5432   |<---ssh -R---->| local :{SSG_DYN} (54201)    |
|                            |               |       |                     |
|                            |               |       v                     |
|                            |               | SSG DinD publishes 54201    |
|                            |               |   -> inner postgres :5432   |
|                            |               |       |                     |
|                            |               |       v                     |
|                            |               | host bind /var/coast-data   |
+----------------------------+               +-----------------------------+
```

### 20.4 `auto_create_db` is always local

Per-instance DB creation for remote consumer coasts still happens on
the local machine, against the SSG's inner postgres, via nested
`docker exec`. No exec ever runs on `coast-service` for SSG services —
the remote is purely a consumer of the tunnel.

### 20.5 Ordering in `handle_remote_run`

[`coast-daemon/src/handlers/run/mod.rs::handle_remote_run`](../coast-daemon/src/handlers/run/mod.rs)
gains one extra step at the front of its pipeline (before the existing
"Starting shared services" phase):

0. Resolve SSG references from the local artifact's Coastfile.
1. If any, ensure SSG is running (auto-start per §11.1, guarded by
   `ssg_mutex`).
2. Run the existing `setup_shared_service_tunnels` with SSG dynamic
   ports on the local side of each pair (§20.2).
3. Run `auto_create_db` locally against the SSG (§20.4).
4. Forward `RunRequest` to `coast-service`, unchanged.

### 20.6 SSG lifecycle respects active remote shadow instances

`coast ssg stop` and `coast ssg rm`:

1. Look up every local shadow instance (`remote_host IS NOT NULL`)
   whose artifact references any SSG service.
2. Refuse the operation (without `--force`) if any such instance is
   running. Message lists them:
   `"SSG is currently serving remote coast 'my-app/dev-1'. Stopping the SSG will break its postgres/redis connectivity. Stop it first, or re-run with --force."`
3. When `--force`: tear down per-coast reverse tunnels first, then
   stop / remove the SSG.

### 20.7 Remote SSGs explicitly out of scope

v1 does not support an SSG on a `coast-service` host. That would
require `coast-service` to grow its own SSG runtime (or a separate
`coast-ssg-service` binary). Track separately; do not introduce
coupling.

## 21. Development approach

Normative. These rules govern every phase.

### 21.1 Design-driven

- Every phase lands in two commits: (a) DESIGN.md deltas and test
  plan first, (b) implementation + tests green.
- No phase begins without updating §0 and the relevant §16 entry.

### 21.2 Integration-test heavy

- Every user-visible behavior is covered by an integration test at
  `integrated-examples/test_ssg_*.sh` (or
  `integrated-examples/ssg/test_*.sh` if we group them later) plus a
  test project under `integrated-examples/projects/coast-ssg-*`.
- Tests are registered in
  [`dindind/integration.yaml`](../dindind/integration.yaml) and
  invoked via [`Makefile`](../Makefile) `run-dind-integration TEST=...`.
- Pattern follows the existing `test_shared_service_ports`,
  `test_volume_shared_services`, `test_cleanup_shared_services_volume`
  suite.

### 21.3 Unit tests everywhere

- Each new module under `coast-ssg/src/` has a `#[cfg(test)] mod tests`
  block.
- Parse errors, compose synthesis, bind-mount translation, port
  allocation, DB migrations, and command construction all have unit
  coverage.
- Rule of thumb: if there is a conditional, there is a unit test.

### 21.4 Phase-gated

Each phase's exit criteria:

- All integration tests listed in §16 for the phase pass in dindind
  (`make run-dind-integration TEST=<name>`).
- `make test` green (workspace-wide `cargo test --workspace`).
- `make lint` clean (`cargo fmt --all -- --check` and
  `cargo clippy --workspace -- -D warnings`).
- No new `#[allow(clippy::...)]` or equivalent suppressions — see
  Ground Rules at the top of this document.
- Progress boxes in §0 ticked in the same commit.

Reviewers MUST reject a PR that ships a clippy suppression even if
every other exit criterion is satisfied.

### 21.5 Test project naming

Every SSG test project under
[`integrated-examples/projects/`](../integrated-examples/projects/)
uses the `coast-ssg-` prefix:

- `coast-ssg-minimal` — one postgres, no bind mount, default config
- `coast-ssg-bind-mount` — host bind mount + permission note
- `coast-ssg-multi-service` — postgres + redis + mongodb
- `coast-ssg-consumer` — consumer Coastfile that uses `from_group = true`
- `coast-ssg-remote` — remote consumer + reverse-tunnel scenario

### 21.6 Indicative integration tests

Registered in their respective phases, not now:

- `test_ssg_build_minimal` — SSG with one postgres; `coast ssg build`
  succeeds; artifact structure correct.
- `test_ssg_run_lifecycle` — `coast ssg run / stop / start / restart / rm`.
- `test_ssg_bind_mount_symmetric` — host path visible with same
  inodes inside the inner postgres.
- `test_ssg_named_volume_persists` — inner named volume survives
  `coast ssg stop`+`start`.
- `test_ssg_consumer_basic` — consumer coast with `from_group = true`
  reaches the SSG postgres.
- `test_ssg_consumer_conflict` — inlined + `from_group = true` fails
  at build.
- `test_ssg_consumer_missing_service` — `from_group = true` for a
  name not in SSG fails clearly.
- `test_ssg_auto_start_on_run` — `coast run` auto-starts the SSG.
- `test_ssg_auto_create_db` — per-instance DB appears inside SSG
  postgres after coast run.
- `test_ssg_port_collision` — two consumer coasts share one SSG
  postgres with no host-port conflict.
- `test_ssg_host_checkout` — `coast ssg checkout postgres` binds
  `localhost:5432`; displaces existing coast owner.
- `test_ssg_remote_reverse_tunnel` — remote coast reaches local SSG
  postgres via reverse SSH tunnel.
- `test_ssg_stop_blocked_by_remote` — `coast ssg stop` refuses while
  remote coast is using it.
- `test_ssg_stop_force_cleans_tunnels` — `--force` tears down tunnels
  and stops.
- `test_ssg_drift_warning` — SSG rebuilt with different image;
  consumer coast warns on run.
- `test_ssg_drift_missing_service` — SSG rebuilt without a referenced
  service; consumer coast fails to run with clear message.

## 22. Terminology cheat sheet

Pinned glossary for context-compacted sessions.

| Term | Meaning |
|---|---|
| SSG | Shared Service Group — the singleton DinD runtime |
| SSG service | A single inner service (postgres, redis, ...) managed by the SSG |
| SSG Coastfile | The top-level `Coastfile.shared_service_groups` TOML file |
| Consumer coast | A regular coast that references SSG services via `[shared_services.<name>] from_group = true` |
| Canonical port | The port apps talk to by name (5432, 6379, ...); unchanged |
| SSG host port | The dynamically allocated host port the SSG publishes for an inner service |
| Inner compose | The `compose.yml` the SSG DinD runs inside itself to start its services |
| Symmetric path | The bind-mount plan in §10.2 — the same host path string on both mount hops |
| Displacement | `coast ssg checkout` taking over a canonical port held by a coast instance |
| Drift | Mismatch between a coast build's recorded SSG reference and the current SSG state (§6.1) |
