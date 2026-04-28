# Remote SSG — coast-service Implementation Companion

> Status: Pre-implementation. Coast-service-side implementation of the
> remote-SSG design captured in
> [`../coast-ssg/REMOTE_DESIGN.md`](../coast-ssg/REMOTE_DESIGN.md).
>
> Read [`../coast-ssg/REMOTE_DESIGN.md`](../coast-ssg/REMOTE_DESIGN.md)
> before this file. That document is the architectural source of
> truth: Coastfile syntax, CLI verbs, pointer semantics, routing
> chains, importer/exporter trait shape. This file is the
> implementation cut for one crate (`coast-service`) — HTTP surface,
> state schema, filesystem layout, host socat, keystore handling, and
> reconciliation.
>
> Companion to [`REMOTE_SPEC.md`](./REMOTE_SPEC.md), which covers
> remote *coasts*. The relationship between these two docs mirrors
> the relationship between `coast-ssg/DESIGN.md` and
> `coast-ssg/REMOTE_DESIGN.md`: REMOTE_SPEC is the original
> coast-service design (remote coasts only); this file extends it to
> include SSG-side handlers.

## Phase tracking

The single source of truth for project status is
[`coast-ssg/REMOTE_DESIGN.md §0`](../coast-ssg/REMOTE_DESIGN.md). Each
section in this file calls out the phase the deliverable belongs to so
contributors can cross-reference, but the canonical checklist is
NOT duplicated here. When a phase ticks a box that affects
coast-service, the checkbox lives in REMOTE_DESIGN.md §0.

## Table of contents

- [§1 Overview](#1-overview)
- [§2 Crate dependency edge](#2-crate-dependency-edge)
- [§3 HTTP surface](#3-http-surface)
- [§4 State schema](#4-state-schema)
- [§5 Filesystem layout](#5-filesystem-layout)
- [§6 Build pipeline](#6-build-pipeline)
- [§7 Run pipeline](#7-run-pipeline)
- [§8 Lifecycle handlers](#8-lifecycle-handlers)
- [§9 Host socat on coast-service](#9-host-socat-on-coast-service)
- [§10 Keystore handling](#10-keystore-handling)
- [§11 Reconciliation on coast-service restart](#11-reconciliation-on-coast-service-restart)
- [§12 Disk management](#12-disk-management)
- [§13 Failure modes specific to coast-service](#13-failure-modes-specific-to-coast-service)
- [§14 Security](#14-security)
- [§15 File map](#15-file-map)
- [§16 Open questions specific to coast-service](#16-open-questions-specific-to-coast-service)
- [§17 Phase-gate inheritance](#17-phase-gate-inheritance)

---

## 1. Overview

This file covers the coast-service crate only. Everything user-visible
(Coastfile syntax, CLI flags, pointer semantics, snapshot bundle
format, importer/exporter trait shape) lives in
[`../coast-ssg/REMOTE_DESIGN.md`](../coast-ssg/REMOTE_DESIGN.md). New
SSG-related code in `coast-service/` belongs here.

The topology, extending the diagram from
[`REMOTE_SPEC.md §Architecture`](./REMOTE_SPEC.md):

```text
LOCAL                                       REMOTE (coast-server)
+------------------------------+            +-------------------------------------------+
| coast-daemon                 |   SSH RPC  | coast-service (:31420)                    |
|   ssg.remote_name shadow     |<==========>|   /ssg/* HTTP handlers                    |
|   ssg_pointers               |            |   ssg / ssg_services / ssg_virtual_ports  |
|   ssg_remote_tunnels         |            |                                           |
|                              |            |   +--------------------------+            |
|                              |   ssh -L   |   | {project}-ssg DinD       |            |
|                              |<==========>|   |  inner postgres/redis/...|            |
|                              |            |   +--------------------------+            |
|                              |            |   remote host socat (per-service)         |
|                              |            |   $COAST_SERVICE_HOME/ssg/<project>/      |
+------------------------------+            +-------------------------------------------+
```

`coast-service` mirrors what `coast-daemon` does for local SSGs,
against its own Docker daemon and its own state.db. The orchestration
code is **its own implementation** under `coast-service/src/ssg/` —
not a re-export of `coast-ssg::runtime::*`. coast-service does NOT
depend on `coast-ssg`. The truly shared pieces (host_socat supervisor,
inner-compose YAML synthesis, manifest types) are lifted into
`coast-docker` and `coast-core` in Phase R-0.5 and consumed from both
sides. See [`../coast-ssg/REMOTE_DESIGN.md`](../coast-ssg/REMOTE_DESIGN.md)
Ground Rule #3 for the full rationale.

## 2. Crate dependency boundaries

The [`coast-ssg/DESIGN.md §4.1`](../coast-ssg/DESIGN.md) layering rule
is **preserved**:

> `coast-service` does **not** depend on `coast-ssg`. This is a
> deliberate constraint.

That rule was originally justified by the v1 non-goal "remote-resident
SSG" ([`§17-6`](../coast-ssg/DESIGN.md), [`§20.7`](../coast-ssg/DESIGN.md)).
This document moves remote SSG into scope, but does NOT collapse the
layering. Instead the truly shared primitives are lifted into lower
crates that both sides already depend on:

| Primitive | Old home | New home (R-0.5) | Why |
|---|---|---|---|
| `host_socat` supervisor | `coast-daemon/src/handlers/ssg/host_socat.rs` | `coast-docker/src/host_socat.rs` | It's a Docker-network primitive (spawns `socat` listening on a host port, forwarding to a Docker-published port). Same posture as Phase 18's lift of `shared_service_routing` to `coast-docker`. |
| `compose_synth` | `coast-ssg/src/runtime/compose_synth.rs` | `coast-docker/src/ssg_compose_synth.rs` | Pure YAML synthesis — no SSG state, no daemon glue. Belongs in the Docker primitives crate. |
| `SsgManifest` and adjacent | `coast-ssg/src/build/artifact.rs` | `coast-core/src/artifact/ssg.rs` | Already protocol-adjacent; both sides round-trip the same JSON shape. |

[`coast-service/Cargo.toml`](./Cargo.toml) gains NO `coast-ssg`
dependency. It already depends on `coast-docker` and `coast-core`,
which is sufficient to consume all three lifted primitives.

`coast-service/src/ssg/` (new directory in Phase R-3) is the
coast-service-local SSG implementation. It re-implements the
lifecycle orchestration (`run_ssg`, `stop_ssg`, etc.) using the
lifted primitives — leaner than `coast-ssg::runtime::lifecycle::*`
because daemon-only concerns (pin resolution, drift checks,
auto-prune across worktrees, doctor) don't apply.

Mark `§17-6` SETTLED in `coast-ssg/DESIGN.md` as SUPERSEDED in the
same PR that lands Phase R-3, but note that the supersede is partial:
remote SSGs are now in scope, but the §4.1 layering rule it
referenced is preserved.

## 3. HTTP surface

All endpoints live under `/ssg/*` on coast-service's existing Axum
router (port 31420). Payloads use the same `SsgRequest` / `SsgResponse`
types from [`coast-core/src/protocol/ssg.rs`](../coast-core/src/protocol/ssg.rs)
that the daemon's CLI wraps.

| Method | Path | Verb | Phase | Maps to (coast-service-local) |
|---|---|---|---|---|
| POST | `/ssg/build` | Build SSG from rsynced staging dir | R-2 | `coast_service::ssg::build::build_ssg` |
| POST | `/ssg/run` | Provision SSG DinD | R-3 | `coast_service::ssg::lifecycle::run` |
| POST | `/ssg/stop` | Stop SSG DinD | R-3 | `coast_service::ssg::lifecycle::stop` |
| POST | `/ssg/start` | Start a stopped SSG | R-3 | `coast_service::ssg::lifecycle::start` |
| POST | `/ssg/restart` | Stop + start | R-3 | `coast_service::ssg::lifecycle::restart` |
| POST | `/ssg/rm` | Remove SSG container, optionally inner volumes | R-3 | `coast_service::ssg::lifecycle::rm` |
| POST | `/ssg/ps` | Per-project SSG status | R-3 | `coast_service::ssg::lifecycle::ps` |
| POST | `/ssg/logs` | Streaming logs from outer DinD or inner service. Chunked streaming over the SSH-tunneled HTTP connection, mirroring the existing pattern in [`coast-service/src/handlers/logs.rs`](./src/handlers/logs.rs) for regular coasts. Supports `--follow` (stream new lines as written) and `--tail N`. Closing the local CLI cleanly cancels the upstream Docker logs stream. Per [REMOTE_DESIGN.md §13.5](../coast-ssg/REMOTE_DESIGN.md), behavior MUST be byte-identical to local SSG `coast ssg logs`. | R-3 | `coast_service::ssg::lifecycle::logs` |
| POST | `/ssg/exec` | Exec into outer DinD or inner service. Bidirectional chunked streaming over the SSH-tunneled HTTP connection (stdin/stdout/stderr), mirroring [`coast-service/src/handlers/exec.rs`](./src/handlers/exec.rs). TTY allocation via the Docker exec API (`AttachStdin: true, Tty: true`). Signal forwarding: Ctrl-C in the local CLI is framed as a SIGINT message that coast-service translates into a `docker kill --signal SIGINT` against the exec instance. Cancellation cleans up the docker exec instance, the SSH channel, and any pending I/O. Per [REMOTE_DESIGN.md §13.5](../coast-ssg/REMOTE_DESIGN.md), behavior MUST be byte-identical to local SSG `coast ssg exec`. | R-3 | `coast_service::ssg::lifecycle::exec` |
| POST | `/ssg/ports` | Virtual + dynamic ports for tunnel argv construction | R-4 | `coast_service::ssg::lifecycle::ports` |
| POST | `/ssg/auto_create_db` | Nested-exec helper for consumer per-instance DB creation | R-4 | `coast_service::ssg::lifecycle::auto_create_db` |
| POST | `/ssg/upstream` | Configure consumer-routing host_socat upstream (Phase R-5; called when consumer location != SSG location) | R-5 | `coast_service::ssg::routing::configure_upstream` |
| POST | `/ssg/upstream/probe` | **Diagnostic-only.** TCP-probe a candidate upstream from this coast-server. Surfaces network-misconfiguration errors at provision time so consumers fail fast instead of hanging. Does NOT drive a routing-variant decision — there is no fallback path. | R-5 | `coast_service::ssg::routing::probe_upstream` |
| POST | `/ssg/export` | Run an exporter; return blob path on coast-service | R-6 | `coast_service::ssg::transfer::run_export` |
| POST | `/ssg/import` | Consume blob from scratch; run importer | R-6 | `coast_service::ssg::transfer::run_import` |
| POST | `/ssg/secrets/clear` | Drop all keystore rows for `ssg:<project>` | R-3 | `keystore.delete_secrets_for_image("ssg:<project>")` |

Each row maps to a function in
[`coast-service/src/ssg/`](./src/ssg/). The HTTP layer in
[`coast-service/src/handlers/ssg.rs`](./src/handlers/ssg.rs) (and
[`ssg_transfer.rs`](./src/handlers/ssg_transfer.rs) for export/import)
is a thin "deserialize → dispatch into `crate::ssg::*` → serialize"
wrapper; orchestration logic lives in `crate::ssg::*`, not in the HTTP
handlers.

Internally, `crate::ssg::*` consumes the R-0.5 lifted primitives:

- `coast_docker::host_socat` for per-`(project, service)` socat
  supervision (spawn / kill / reconcile / collision-rebind).
- `coast_docker::ssg_compose_synth` for inner-compose YAML synthesis
  during build.
- `coast_docker::dind` for outer DinD container creation.
- `coast_core::artifact::ssg::SsgManifest` for reading + writing the
  artifact manifest.
- `coast_secrets::keystore` for the per-run secret materialization
  (encrypted blob shipped from daemon, key passed in-band; see §10).

The SSG runtime code is **NOT re-exported** from `coast-ssg`. Each
side has its own implementation tuned to its responsibilities.

The `/ssg/build`, `/ssg/run`, `/ssg/logs`, and `/ssg/export` endpoints
support streaming progress events via the same `BuildProgressEvent`
channel that coast-service already uses for `/run` and `/build`.

## 4. State schema

coast-service grows its own SSG state tables, identical in shape to
the daemon's. Migration lives in
[`coast-service/src/state/mod.rs`](./src/state/mod.rs); the per-table
CRUD lives in [`coast-service/src/state/ssg.rs`](./src/state/ssg.rs)
(new file).

```sql
CREATE TABLE IF NOT EXISTS ssg (
    project       TEXT PRIMARY KEY,
    container_id  TEXT,
    status        TEXT NOT NULL,
    build_id      TEXT,
    created_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ssg_services (
    project              TEXT NOT NULL,
    service_name         TEXT NOT NULL,
    container_port       INTEGER NOT NULL,
    dynamic_host_port    INTEGER NOT NULL,
    status               TEXT NOT NULL,
    PRIMARY KEY (project, service_name, container_port)
);

CREATE TABLE IF NOT EXISTS ssg_virtual_ports (
    project              TEXT NOT NULL,
    service_name         TEXT NOT NULL,
    container_port       INTEGER NOT NULL,
    port                 INTEGER NOT NULL,
    created_at           TEXT NOT NULL,
    PRIMARY KEY (project, service_name, container_port)
);

-- Phase R-5: per-(project, service, container_port) host_socat upstream
-- descriptor. Tells coast-service's host_socat what to forward to.
-- Two SOURCES:
--   "ssg" — coast-service is hosting the SSG; upstream is its own
--           inner SSG dyn port (127.0.0.1:<dyn>).
--   "consumer_direct" — coast-service is hosting a consumer routing
--                       to an SSG on a different coast-server;
--                       upstream is "<other-coast-server>:<vport>".
--
-- The same-coast-server case (REMOTE_DESIGN.md §12.1) does NOT create
-- a third row — it reuses the "ssg" row directly because the consumer
-- and the SSG share the same host_socat. Routing-variant attribution
-- for that case lives in the daemon's ssg_consumer_routing table
-- (variant = "remote_same"), not here.
--
-- See REMOTE_DESIGN.md §12.
CREATE TABLE IF NOT EXISTS ssg_socat_upstreams (
    project              TEXT NOT NULL,
    service_name         TEXT NOT NULL,
    container_port       INTEGER NOT NULL,
    source               TEXT NOT NULL,          -- "ssg" | "consumer_direct"
    upstream_addr        TEXT NOT NULL,          -- "host:port" string
    created_at           TEXT NOT NULL,
    PRIMARY KEY (project, service_name, container_port, source)
);
```

The CRUD trait is named `SsgServiceStateExt` and is defined locally
in [`coast-service/src/state/ssg.rs`](./src/state/ssg.rs). It is NOT
shared with coast-daemon (the daemon's `SsgStateExt` carries
daemon-only methods like pointer/tunnel rows that don't apply on
coast-service). The two traits have a deliberately overlapping shape
for the rows that exist on both sides (project, services, virtual
ports), but they are distinct types — there is no shared trait
crate.

`auto_prune` semantics, virtual-port allocation (matching the daemon's
Phase 26+28 algorithm), and host-socat reconciliation (matching Phase
27+28+31) carry forward conceptually but are re-implemented in
`crate::ssg::*` against the lifted `coast_docker::host_socat`
primitive. Pin handling (`DESIGN.md` Phase 16) is daemon-only — pins
are resolved on the daemon side before `/ssg/run` is called, so
coast-service receives a concrete `build_id` and never needs the pin
logic.

## 5. Filesystem layout

Everything coast-service owns for SSGs lives under `$COAST_SERVICE_HOME`
(default `/data`):

```text
$COAST_SERVICE_HOME/
  ssg/
    <project>/
      staging/                           <-- rsynced Coastfile + secret extractor inputs (R-2)
                                             wiped at end of each successful build
      builds/<id>/                       <-- canonical build artifact (R-2)
        manifest.json
        ssg-coastfile.toml
        compose.yml
        images/                          <-- cached image tarballs (deduped to ../image-cache/)
      runs/<project>/                    <-- per-run scratch (R-3)
        compose.override.yml             <-- secrets materialized here at run time (R-3)
        secrets/<basename>               <-- file-secret payloads (mode 0600)
        snapshots/<id>/                  <-- transient export/import scratch (R-6)
  image-cache/                           <-- shared across coast + SSG builds (existing)
  workspaces/<project>/<instance>/       <-- existing (remote coasts)
  keystore.db                            <-- encrypted blob synced from daemon (R-3)
                                             key NEVER persists; see §10
  keystore.key                           <-- NOT created on coast-service (intentional absence)
```

The `ssg/<project>/runs/<project>/` nesting mirrors the daemon's
existing layout at `~/.coast/ssg/<project>/runs/<project>/`. The
double-`<project>` is awkward but it matches the daemon convention
documented in [`coast-ssg/DESIGN.md §33.3`](../coast-ssg/DESIGN.md).
The compose-override file shape and bind-mount path
(`/coast-runtime/`) are identical on both sides, so a service
inspector debugging on either machine sees the same layout.

`crate::ssg::secrets_inject::materialize_secrets` re-implements the
override renderer using the lifted `coast_core::artifact::ssg::SsgManifest`
type. The implementation is a leaner mirror of
[`coast-ssg/src/runtime/secrets_inject.rs`](../coast-ssg/src/runtime/secrets_inject.rs)
since coast-service doesn't need the daemon-side concerns (build
hash invalidation, doctor integration).

Build artifacts on coast-service follow `auto_prune` keep-N (same
default as local). Pinned builds are NOT a concern on coast-service —
pin resolution happens on the daemon side, so coast-service receives
the concrete `build_id` to keep / use directly.

## 6. Build pipeline

Phase R-2 lights up `POST /ssg/build`. The daemon does most of the
parsing work locally (it has the user's Coastfile in hand and is
running an architecture coast-service can't always introspect);
coast-service handles the image pull + manifest write + compose synth.

Sequence:

1. **Daemon side.** Daemon parses the SSG Coastfile locally (validates
   syntax, rejects `HostBindMount` when `[ssg].remote` is set, runs
   `[secrets.*]` extractors against the developer's machine).
   Encrypted keystore rows land in `~/.coast/keystore.db` under
   `coast_image = "ssg:<project>"`.
2. **Daemon side.** Daemon rsyncs the Coastfile (TOML bytes) and any
   `[secrets]` file-inject extractor outputs to
   `coast-service:$COAST_SERVICE_HOME/ssg/<project>/staging/`. The
   encrypted keystore blob is rsynced separately to
   `$COAST_SERVICE_HOME/keystore.db` (see §10).
3. **Daemon side.** Daemon issues `POST /ssg/build` with the staging
   relative path. The encrypted keystore key is NOT included in the
   build call — it's only needed at run time.
4. **coast-service side.** [`handlers/ssg.rs::handle_build`](./src/handlers/ssg.rs)
   parses the Coastfile from the staging dir (using
   `coast_core::artifact::ssg::SsgCoastfile` — wait, the Coastfile parser
   itself stays in `coast-ssg`. coast-service receives a pre-parsed
   `SsgCoastfile`-shape via the `BuildRequest` body instead. Daemon
   parses, serializes the validated form to JSON, ships it; coast-service
   deserializes. This avoids re-importing the parser into
   `coast-service`).
5. **coast-service side.** [`crate::ssg::build::build_ssg`](./src/ssg/build.rs)
   runs the pipeline:
   - For each `[shared_services.<svc>]`: call
     `coast_docker::image_cache::pull_and_cache_image` (already
     shared) to populate `$COAST_SERVICE_HOME/image-cache/`.
   - Synthesize `compose.yml` via
     `coast_docker::ssg_compose_synth::synthesize_inner_compose`
     (lifted in R-0.5).
   - Write `manifest.json` of shape
     `coast_core::artifact::ssg::SsgManifest` (lifted in R-0.5).
   - Auto-prune older builds (keep N).
6. **coast-service side.** Returns `SsgManifest` + `build_id` in the
   response. Wipes the staging dir on success.
7. **Daemon side.** Stores the manifest at
   `~/.coast/ssg/<project>/remote-builds/<remote>/<id>/manifest.json`
   for `coast ssg ps` / `ls` UX. Image tarballs are NOT mirrored
   locally (different from the regular `coast build --type remote`
   flow, which DOES round-trip image tarballs — SSG builds don't,
   because the SSG only runs on the side that built).

Cache namespace: `$COAST_SERVICE_HOME/image-cache/` is shared between
remote coasts and remote SSGs, both using the same
`coast_docker::image_cache::pull_and_cache_image` helper. Tarball
naming convention is identical to the daemon's local cache, so a
hypothetical future "image-cache federation" feature could dedupe
across hosts without re-keying.

**Why the daemon parses, not coast-service.** Avoids importing the
SSG Coastfile parser (which lives in `coast-ssg`) into coast-service
and crossing the §4.1 boundary. The parser is daemon-only; the
serialized validated form (`SsgCoastfile` JSON) is what flows over
the wire. If we ever want coast-service to parse Coastfile syntax
directly, the parser can be lifted to `coast-core` in a future
phase.

## 7. Run pipeline

Phase R-3 wires `POST /ssg/run`. The orchestration lives in
[`crate::ssg::lifecycle::run`](./src/ssg/lifecycle.rs) — a leaner
re-implementation of the local
[`coast_ssg::runtime::lifecycle::run_ssg_with_build_id`](../coast-ssg/src/runtime/lifecycle.rs)
that consumes the lifted primitives directly.

Sequence:

1. **Resolve build_id.** Body carries the concrete `build_id`
   (already pin-resolved on the daemon side; coast-service does NOT
   re-resolve pins). Hard-error if the build dir doesn't exist
   under `$COAST_SERVICE_HOME/ssg/<project>/builds/<id>/`.
2. **Read manifest.** Load `manifest.json` into a
   `coast_core::artifact::ssg::SsgManifest` (lifted in R-0.5).
3. **Allocate dynamic outer ports.** Use the existing
   `coast_service::port_manager::allocate_dynamic_ports` (already
   present for regular coasts) to assign one per service's
   `container_port`.
4. **Allocate / reuse virtual ports.** Walk `ssg_virtual_ports`,
   allocating new entries from
   `$COAST_VIRTUAL_PORT_BAND_START..$COAST_VIRTUAL_PORT_BAND_END`
   (defaults 42000-43000) for any `(service, container_port)` not
   yet recorded. Re-use existing entries when present (stable across
   rebuilds — same posture as daemon-side virtual ports). The
   remote-side virtual port does not need to match the daemon-side
   one; they're independent allocations.
5. **Create outer DinD.** Use
   `coast_docker::dind::build_dind_config` + the existing
   `coast_service::Runtime::create_coast_container` (or a new SSG
   variant — see §15). Bind-mount the build artifact at
   `/coast-artifact/`, the per-run scratch dir at `/coast-runtime/`.
6. **Wait for inner daemon, load images, compose up.** Each step is
   a thin call against `bollard::Docker` plus `docker exec` shells
   into the outer container. The argv builders (`docker compose -f
   ... up -d`, etc.) are simple enough to inline; if they grow,
   lift to `coast-docker`.
7. **Materialize secrets.** If the manifest declares any
   `[secrets.*]` blocks, the daemon-side request body carries the
   in-memory keystore decryption key; coast-service decrypts in
   process via `coast_secrets::keystore`, writes
   `$COAST_SERVICE_HOME/ssg/<project>/runs/<project>/compose.override.yml`,
   layers it onto the compose-up argv. See §10.
8. **Spawn / refresh remote host socats.** Call
   `coast_docker::host_socat::reconcile_project(project, &state)`
   (lifted in R-0.5). Same idempotent argv-swap logic as the daemon.
9. **Apply state.** Write `ssg.status = 'running'`,
   `ssg_services.dynamic_host_port`, return the response.

Streaming progress events use the same 7-step plan as local
([`coast-ssg/src/runtime/lifecycle.rs`](../coast-ssg/src/runtime/lifecycle.rs)
`RUN_STEPS`); the SPA's run modal renders identically regardless of
whether the SSG is local or remote. The plan strings live in
[`crate::ssg::lifecycle`](./src/ssg/lifecycle.rs) — duplicated from
the daemon side. v1 keeps them in sync by convention; if drift
becomes a problem, lift the plan strings into `coast-core::progress`.

## 8. Lifecycle handlers

The remaining lifecycle verbs are direct calls into
`crate::ssg::lifecycle::*`:

| Verb | Body extras | Calls |
|---|---|---|
| `/ssg/stop` | `force: bool` | `lifecycle::stop` (inner compose down + outer container stop), then writes `status = "stopped"` |
| `/ssg/start` | — | `lifecycle::start` reusing `dynamic_host_port`s from state, refreshing host socat upstreams |
| `/ssg/restart` | — | `stop` then `start` (no special handling) |
| `/ssg/rm` | `with_data: bool` | `lifecycle::rm` removes container, optionally inner named volumes, clears `ssg_services` rows and (when `with_data`) `ssg_virtual_ports` rows |

Coast-service-specific behavior:

- The `force` semantics in `DESIGN.md §20.6` (refuse if active
  consumers exist) are evaluated **on the daemon side** before the
  request even reaches coast-service. By the time `/ssg/stop` is
  called, the daemon has already validated. coast-service trusts
  the request.
- `/ssg/rm` cleans up coast-service's own resources: container,
  inner volumes (when `with_data`), `ssg_*` state rows, build
  artifacts older than `auto_prune` keep-N. The daemon's
  `ssg_remote_tunnels` rows are torn down by the daemon, not by
  coast-service.
- Each function operates on a `&bollard::Docker` (passed in from
  `AppState`) and a `&dyn SsgServiceStateExt`. No `SsgDockerOps`
  trait abstraction — coast-service's lifecycle is small enough
  that direct bollard calls are clearer than going through a
  mock-friendly trait. Unit tests for `crate::ssg::lifecycle`
  exercise the pure helpers (port allocation, plan-string
  generation); end-to-end behavior is covered by integration tests
  via the dindind harness.

## 9. Host socat on coast-service

Phase R-4 + R-5 light up the consumer routing path. coast-service's
host_socat plays one of two roles depending on what's running on
this coast-server:

1. **SSG-side (R-3).** When coast-service hosts the SSG itself, a
   socat listens on `<vport>` and forwards to `127.0.0.1:<ssg_dyn>`.
   This is the original §11 role.
2. **Consumer-routing direct (R-5 §12.2).** When coast-service hosts
   a consumer whose SSG lives on a *different* coast-server, a socat
   listens on `<vport>` and forwards to
   `<other-coast-server>:<vport>` over plain TCP. The daemon stamped
   the upstream string at consumer-provision time.

**Same-coast-server case (REMOTE_DESIGN.md §12.1) reuses role 1.**
When the consumer and the SSG share a coast-server, the consumer's
in-DinD socat lands directly on the same SSG-side host socat. No
second socat, no `consumer_local` row in `ssg_socat_upstreams` —
routing-variant attribution lives entirely in the daemon's
`ssg_consumer_routing` table (variant = `remote_same`).

The supervisor module is shared via the R-0.5 lift:

- **Lift target.** [`coast-daemon/src/handlers/ssg/host_socat.rs`](../coast-daemon/src/handlers/ssg/host_socat.rs)
  moves to `coast-docker/src/host_socat.rs` in Phase R-0.5. Both
  daemon and coast-service depend on `coast-docker` already, so
  consuming the lifted module costs nothing. The original location
  in `coast-daemon` is deleted; daemon imports from `coast_docker`.
- **Module API unchanged.** `spawn_or_update`, `kill`,
  `reconcile_project`, `reconcile_all` keep their current
  signatures. They take a `SocketAddr`-shaped upstream string;
  whether the upstream is `127.0.0.1:5432` or
  `coast-server-B:42001` is opaque to the supervisor.
- **Pidfile / log paths.** Path resolution is parameterized via a
  `SocatPaths` struct. The daemon constructs one from
  `coast_home()`; coast-service constructs one from
  `service_home()`. Default lives under `$ROOT/socats/{project}--{service}.{pid,log,argv}`.
- **Preflight.** The daemon's `preflight::check_socat_available`
  also moves to `coast-docker::host_socat::check_available()`. The
  coast-service Dockerfile adds `socat` to its apt install list; a
  startup check in [`coast-service/src/lib.rs`](./src/lib.rs) main
  calls the lifted helper before the Axum router binds.
- **Reachability probe (R-5).** Before spawning a
  `consumer_direct` socat, coast-service performs a TCP probe
  against `<other-coast-server>:<vport>`. Failure surfaces as a
  structured error in the `/ssg/run` response so the daemon can
  hard-error the consumer's `coast run` with the §19 remediation
  message.

The collision-rebind path (Phase 28's `RebindNotice`) carries forward
identically — same module, same logic. coast-service emits a
`RebindNotice` in its `SsgResponse` when a virtual port had to be
reallocated; the daemon forwards it to the user.

## 10. Keystore handling

Per [`../coast-ssg/REMOTE_DESIGN.md §13.3`](../coast-ssg/REMOTE_DESIGN.md)
and [`§23 Open Question 3`](../coast-ssg/REMOTE_DESIGN.md), the
recommendation is **A: extract on the daemon, ship encrypted to
coast-service.** Implementation:

1. **Build time (R-2 + R-3).** Daemon's `coast ssg build --remote
   <name>` runs the local extractor pipeline FIRST against the
   developer's machine credentials (env / 1password / keychain /
   etc.). Encrypted rows land in `~/.coast/keystore.db` under
   `coast_image = "ssg:<project>"`.
2. **Build-finalize.** Daemon rsyncs `~/.coast/keystore.db` to
   `coast-service:$COAST_SERVICE_HOME/keystore.db`. **Only the
   encrypted blob is shipped.** The keystore key
   (`~/.coast/keystore.key`) NEVER leaves the daemon.
3. **Run time (R-3).** Daemon issues `POST /ssg/run` with the
   keystore key bytes in the request body (in-memory only,
   transmitted over the SSH-tunneled HTTP connection). coast-service
   decrypts in-process via
   `coast_secrets::keystore::Keystore::open_with_inline_key(...)`,
   passes decrypted material into
   `crate::ssg::secrets_inject::materialize_secrets`, drops the key
   from memory before returning. The materialize logic is a leaner
   re-implementation of the daemon's
   [`coast-ssg/src/runtime/secrets_inject.rs`](../coast-ssg/src/runtime/secrets_inject.rs)
   — the override-file YAML shape and bind-mount target
   (`/coast-runtime/`) match exactly so a service inspector sees
   identical layout on both sides.
4. **No long-term key state.** coast-service does NOT write the key
   to disk. After a coast-service restart, existing SSG containers
   keep running (they read the materialized override file from the
   bind mount); a fresh `coast ssg run` after restart requires the
   daemon to re-supply the key.

Daemon side, the rsync of the encrypted blob happens once per build
finalize (the same trigger that ships the artifact). Subsequent
`coast ssg run` calls do NOT re-rsync the keystore — the encrypted
blob on coast-service is already current.

The encryption boundary stays exactly where it is for local SSGs:
extractor → encrypted at rest → decrypted in-memory only when
materializing into the inner container. Coast-service is just one
more node in that chain.

`SecretsClear` (Phase 33's `coast ssg secrets clear`) follows the
same path: when the resolved location is remote, the daemon forwards
`POST /ssg/secrets/clear` to coast-service, which calls
`keystore.delete_secrets_for_image("ssg:<project>")` on its local
encrypted blob. The daemon also clears its own keystore for symmetry.

## 11. Reconciliation on coast-service restart

coast-service restarts (binary upgrade, container reboot, host
reboot) are an existing concern — `REMOTE_SPEC.md` describes the
remote-coast version. SSG-specific extension:

1. **State.db survives.** Inner DinD containers and named volumes
   survive across restarts (Docker outlives the coast-service
   process). state.db rows for `ssg`, `ssg_services`,
   `ssg_virtual_ports`, and `ssg_socat_upstreams` are intact.
2. **Reconciliation pass.** On boot, coast-service runs
   `host_socat::reconcile_all(state)` which iterates every project
   with `ssg.status = 'running'`, joins `ssg_services` ×
   `ssg_virtual_ports` × `ssg_socat_upstreams`, and respawns any
   missing host socats with the upstream addr from
   `ssg_socat_upstreams.upstream_addr`. The supervisor doesn't
   care whether the upstream is loopback (SSG-side / consumer-local)
   or a remote hostname (consumer-direct) — it just opens a TCP
   listener and forks `socat`. Same reconciliation as the daemon
   (`DESIGN.md §31` Phase 32 fix).
3. **Tunnels respawn from the daemon.** Daemon-side `ssh -L` and
   `ssh -R` processes live on the daemon side. Daemon-side
   `restore_ssg_remote_tunnels` walks `ssg_remote_tunnels` and
   respawns any with dead pids. The §12.2 direct cross-server
   path has no daemon-side tunnel — the upstream addr persists in
   coast-service-A's `ssg_socat_upstreams` and survives a
   coast-service-A restart, a coast-service-B restart, and a daemon
   restart equally well. Restart on any side eventually converges.
4. **In-flight runs/builds.** A run or build that was streaming
   when coast-service restarted is lost. Daemon's HTTP client gets
   an EOF; user retries. Idempotency: `ssg run` on an
   already-running container short-circuits via the
   `AlreadyRunning` branch (Phase 32 fix).
5. **Materialized secrets.** Per §10, the override file at
   `$COAST_SERVICE_HOME/ssg/<project>/runs/<project>/compose.override.yml`
   was written at the previous run-time. Inner DinD continues to
   read it. Restart does not lose the materialized secrets;
   coast-service does not need the key in this state.

## 12. Disk management

Disk usage on coast-server with one or more SSGs:

| Resource | Approx size | Cleaned by |
|---|---|---|
| SSG outer DinD container | ~200MB image + per-run state | `coast ssg rm` |
| SSG inner volumes (named) | service-dependent (postgres, redis…) | `coast ssg rm --with-data` |
| SSG host bind mounts | rejected when `[ssg].remote` set; N/A | N/A |
| Build artifacts under `ssg/<project>/builds/<id>/` | ~10-100MB per build, image tarballs dedup'd | `auto_prune` (keep 5) + pinned-aware |
| Image cache `image-cache/` | shared with regular coast builds | existing prune |
| Run scratch `ssg/<project>/runs/<project>/` | < 1MB usually | wiped on next run |
| Snapshot scratch `ssg/<project>/runs/<project>/snapshots/<id>/` | snapshot-dependent | wiped after each export/import op |

`coast remote prune <name>` ([`coast-service/src/handlers/prune.rs`](./src/handlers/prune.rs))
extends to:

- Identify orphaned SSG containers (in Docker but not in `ssg`
  table) and remove.
- Identify orphaned build dirs (on disk but not referenced by any
  `ssg.build_id`, subject to `auto_prune` keep-N) and remove.
- Identify orphaned snapshot scratch dirs (on disk but no in-flight
  operation) and remove.
- Surface aggregated free-space numbers in the prune report.

The existing prune logic for regular coast workspaces and DinD
volumes is unchanged.

## 13. Failure modes specific to coast-service

| Failure | Behavior |
|---|---|
| SSH RPC times out mid-build | coast-service kills any in-flight bollard ops, leaves the staging dir behind for retry |
| Daemon disconnects mid-run | coast-service finishes the run; daemon may have moved on. coast-service's state remains authoritative; daemon reconciles on next `/ssg/ps` |
| coast-service binary upgrade with running SSGs | Containers persist (Docker), state.db preserved, host socats respawn from sqlite (§11). Same posture as remote coasts |
| Inner DinD daemon crash (rare, Docker bug) | `health-check` loop in the SSG container restarts the inner daemon. Outer coast-service is unaffected; consumer connections RST until inner daemon recovers |
| Disk full during build | Build fails; daemon retries after operator clears space. `coast remote prune <name>` is the operator's primary tool |
| Disk full during run (tmpfs / log overflow) | Inner services may crash; coast-service surfaces the underlying Docker error verbatim. Operator clears space, daemon retries `/ssg/start` |
| Two daemons issue conflicting SSG ops simultaneously | coast-service has its own `ssg_mutex` analog (process-global tokio Mutex in `AppState`). Second daemon waits in line. Both succeed if neither requires destructive cleanup |
| Daemon ships a stale keystore blob (key newer than blob) | `Keystore::open_with_inline_key` fails decryption; coast-service surfaces an error. Daemon re-rsyncs the blob and retries |

## 14. Security

- **`GatewayPorts clientspecified`** is required on coast-server's
  sshd config for both remote-coast reverse tunnels (existing
  REMOTE_SPEC.md requirement) and remote-SSG `ssh -L` from the
  daemon side. Already documented in REMOTE_SPEC.md; no new
  requirement.
- **No keystore key on coast-server.** §10 covers the rationale and
  flow.
- **Snapshot scratch dirs are wiped.** `coast ssg export` and `import`
  use a fresh `snapshots/<id>/` dir per operation, removed at the
  end. A failed export leaves bytes behind that future `coast remote
  prune` will reap.
- **One SSH user per coast-server.** Multi-tenant separation between
  developers sharing a coast-server is out of scope. v1 assumes the
  developers trust each other (or each has their own
  coast-server). Document.
- **Filesystem permissions on `$COAST_SERVICE_HOME/ssg/<project>/`.**
  coast-service runs as root inside its container; the bind-mounted
  `/data` is owned by the SSH user on the host. The
  `make_world_writable` pattern from
  [`coast-service/src/handlers/run.rs`](./src/handlers/run.rs) is
  reused for SSG paths so the daemon's rsync operations don't
  fail on permission errors.

## 15. File map

Every coast-service file touched by this design, with one-line
rationale per file:

- [`coast-service/Cargo.toml`](./Cargo.toml) — NO `coast-ssg`
  dependency (deliberately preserved per Ground Rule #3 in
  REMOTE_DESIGN.md). Existing `coast-docker` and `coast-core`
  dependencies are sufficient to consume the lifted primitives.
- [`coast-service/src/ssg/mod.rs`](./src/ssg/mod.rs) — NEW. Module
  root for the coast-service-local SSG implementation. Phase R-0
  scaffolds the empty module; subsequent phases fill it in.
- [`coast-service/src/ssg/lifecycle.rs`](./src/ssg/lifecycle.rs) —
  NEW. `run`, `stop`, `start`, `restart`, `rm`, `ps`, `logs`,
  `exec`, `ports`, `auto_create_db` orchestrators. Phase R-3.
- [`coast-service/src/ssg/build.rs`](./src/ssg/build.rs) — NEW.
  Image pull + manifest write + compose synth. Consumes
  `coast_docker::ssg_compose_synth` + `coast_core::artifact::ssg`.
  Phase R-2.
- [`coast-service/src/ssg/secrets_inject.rs`](./src/ssg/secrets_inject.rs)
  — NEW. Decrypt + materialize override.yml. Phase R-3.
- [`coast-service/src/ssg/routing.rs`](./src/ssg/routing.rs) — NEW.
  `configure_upstream` and `probe_upstream` for the §12.2 direct
  cross-server path. Phase R-5.
- [`coast-service/src/ssg/transfer.rs`](./src/ssg/transfer.rs) — NEW.
  Exporter/importer registry; instantiates own builtins. Phase R-6.
- [`coast-service/src/ssg/paths.rs`](./src/ssg/paths.rs) — NEW. Path
  resolution under `$COAST_SERVICE_HOME/ssg/<project>/`. Phase R-2.
- [`coast-service/src/handlers/ssg.rs`](./src/handlers/ssg.rs) — NEW.
  Thin HTTP route layer dispatching into `crate::ssg::*`. Phase R-2
  onwards.
- [`coast-service/src/handlers/ssg_transfer.rs`](./src/handlers/ssg_transfer.rs)
  — NEW. Export / import HTTP handlers. Phase R-6.
- [`coast-service/src/handlers/mod.rs`](./src/handlers/mod.rs) — adds
  `pub mod ssg; pub mod ssg_transfer;` declarations.
- [`coast-service/src/state/ssg.rs`](./src/state/ssg.rs) — NEW.
  `SsgServiceStateExt` trait + impl on coast-service's `StateDb`.
  Phase R-3.
- [`coast-service/src/state/mod.rs`](./src/state/mod.rs) — adds the
  three SSG-table migrations.
- [`coast-service/src/server.rs`](./src/server.rs) — wires the new
  routes onto the Axum router under `/ssg/*`.
- [`coast-service/src/lib.rs`](./src/lib.rs) — preflight check for
  `socat` (calls `coast_docker::host_socat::check_available`);
  reconciliation call on startup.
- `coast-docker/src/host_socat.rs` — LIFTED in Phase R-0.5 from
  `coast-daemon/src/handlers/ssg/host_socat.rs`. Consumed by both
  daemon and coast-service.
- `coast-docker/src/ssg_compose_synth.rs` — LIFTED in Phase R-0.5
  from `coast-ssg/src/runtime/compose_synth.rs`.
- `coast-core/src/artifact/ssg.rs` — LIFTED in Phase R-0.5 from
  `coast-ssg/src/build/artifact.rs` (the manifest types).
- [`Dockerfile.coast-service`](../Dockerfile.coast-service) — apt
  install adds `socat`.

## 16. Open questions specific to coast-service

These are coast-service-scoped follow-ups; user-visible open
questions live in
[`../coast-ssg/REMOTE_DESIGN.md §23`](../coast-ssg/REMOTE_DESIGN.md).

1. **(SETTLED — pre-R-0.5)** **Where does the lifted `host_socat`
   supervisor live?** Settled on `coast-docker::host_socat`. See
   [`../coast-ssg/REMOTE_DESIGN.md §23 Open Question 11`](../coast-ssg/REMOTE_DESIGN.md)
   for the full rationale. Short version: it's a Docker-network
   primitive, not SSG-specific; same posture as Phase 18's lift of
   `shared_service_routing` to `coast-docker`.
2. **Should coast-service keep its own keystore key?** Default: no
   (per §10). Open: is there a worth-it threat model where
   coast-service holds a per-host key plus the daemon's blob is
   re-encrypted at rsync time? Probably no — the daemon → coast-service
   trust relationship is already the security boundary.
3. **Does coast-service grow its own `coast ssg ls`-equivalent
   debug verb?** Probably no in v1; daemon is the single user-facing
   CLI. coast-service exposes `/ssg/ls` for the daemon to call but
   doesn't have a CLI of its own. If operators need direct access,
   `curl http://localhost:31420/ssg/ls -d '{"project":""}'` works
   from coast-server's shell.
4. **Should `/ssg/build` accept image tarballs in the request body
   instead of pulling fresh on coast-service?** Could speed up
   builds when the daemon already has the images cached. v1: pull
   fresh; coast-service has its own image cache. Revisit if build
   times are a problem.
5. **Multi-version socat on coast-server.** macOS Docker Desktop
   ships an old socat; Linux distros vary. Are there flag
   compatibility issues we'd hit at the supervisor level?
   Daemon-side `host_socat` already runs on macOS and Linux without
   issue; reuse should inherit the same compatibility. Test on
   Amazon Linux 2 + Ubuntu 22 + Debian 12 in R-3 acceptance.
6. **(SETTLED — pre-R-1)** **Single `ssg_transfer` registry home.**
   Settled on `coast-docker/src/ssg_transfer/` per
   [`../coast-ssg/REMOTE_DESIGN.md §15.1`](../coast-ssg/REMOTE_DESIGN.md).
   Both daemon-side `coast-ssg/src/transfer/` (orchestration) and
   coast-service-side `crate::ssg::transfer` (orchestration) import
   the same registry from `coast-docker`. The trait + 6 builtins
   exist exactly once in the workspace; only orchestration glue is
   per-side. No drift possible by construction.

## 17. Phase-gate inheritance

Coast-service-side work for Remote SSG inherits the same phase-exit
gate machinery defined in
[`../coast-ssg/REMOTE_DESIGN.md §27`](../coast-ssg/REMOTE_DESIGN.md):

- §27.1 Design-driven (two-commit phases: design + impl).
- §27.2 Integration-test heavy
  (`integrated-examples/test_remote_ssg_*.sh` registered in
  [`dindind/integration.yaml`](../dindind/integration.yaml)).
- §27.3 Unit tests everywhere (each new module under
  `coast-service/src/ssg/` ships with `#[cfg(test)] mod tests`).
- §27.4 Phase-gated (`make test`, `make lint` clean, **NO clippy
  suppressions added**). Reviewers MUST reject coast-service PRs
  that ship a clippy suppression even if every other exit
  criterion is satisfied.
- §27.5 Test naming: `test_remote_ssg_*.sh` for tests that touch
  coast-service; `test_ssg_*.sh` for purely local tests.
- §27.6 Acceptance gate: `make run-dind-integration TEST=<each>` for
  every test listed in REMOTE_DESIGN.md §0 for the phase.

This file does NOT duplicate the §27 content — REMOTE_DESIGN.md
remains the single source of truth. coast-service-specific test
projects under `integrated-examples/projects/coast-remote-ssg-*/`
are listed in REMOTE_DESIGN.md §0's per-phase test enumeration.
