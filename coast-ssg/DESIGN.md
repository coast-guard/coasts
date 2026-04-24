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
- [§23 Post-v1 correction: per-project SSG](#23-post-v1-correction-per-project-ssg) — **READ THIS FIRST.** §§1-22 describe the original singleton-per-host model. That model was wrong. §23 is the current truth.

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
- [x] `coast ssg checkout [service | --all]` and `uncheckout`
- [x] `ssg_port_checkouts` writes + daemon-restart recovery
- [x] Displacement of a coast-instance's canonical port with a clear warning
- [x] `coast ssg ports` shows `(checked out)` annotation
- [x] Integration test: `test_ssg_host_checkout`
- [x] Integration test: `test_ssg_checkout_displaces_instance`

### Phase 7 — SSG reference in coast build manifest
- [x] `coast build` embeds an `ssg` block in `manifest.json` when any `from_group = true` service exists
- [x] `coast run` validates drift: match / same-image warn / missing-service error
- [x] Integration test: `test_ssg_drift_warning`
- [x] Integration test: `test_ssg_drift_missing_service`

### Phase 8 — Docs and polish
- [x] `docs/shared_service_groups/` top-level folder (README + 6 topic pages: BUILDING/LIFECYCLE/VOLUMES/CONSUMING/CHECKOUT/CLI); see §17 SETTLED #26 for the deviation from the original single-page concepts_and_terminology intent
- [x] `docs/coastfiles/SHARED_SERVICE_GROUPS.md`
- [x] `docs/concepts_and_terminology/SHARED_SERVICES.md` updated with "See also SSG"
- [x] Host-volume migration recipe documented (`docs/shared_service_groups/VOLUMES.md`)
- [x] `coast ssg doctor` (warns on likely permission mismatches); see §17 SETTLED #27
- [x] `docs/coastfiles/README.md` reference table updated
- [x] `docs/doc_ordering.txt` + `coast-guard/src/components/DocsSidebar.tsx` + `coast-guard/src/locales/en.json` updated for the new section

### Phase 9 — Audit fixes and coverage
Driven by the DESIGN.md §5-§14 audit after Phase 8 landed. Every
Phase 1-8 feature was already implemented; the audit surfaced 13
divergences from the normative text. Phase 9 fixed 6 of them in
code, documented 7 via §17 SETTLED entries, and backfilled unit
tests to raise coverage on the critical SSG files above 70%.

Code fixes:
- [x] `coast ssg build` honors global `--working-dir` (SETTLED #31)
- [x] `coast ssg checkout <SERVICE>` positional form (SETTLED #32)
- [x] `coast build` hard-errors at build time when `from_group` refs exist but no SSG build (SETTLED #33)
- [x] Consumer `auto_create_db = false` as explicit disable override (SETTLED #34)
- [x] `SsgStarting` fires before dispatch; `SsgStarted` after (SETTLED #35)
- [x] `coast ssg ps` merges live `ssg_services` + container status (SETTLED #36)

Coverage infrastructure:
- [x] [`coast-ssg/src/docker_ops.rs`](./src/docker_ops.rs) defines `SsgDockerOps` trait + `BollardSsgDockerOps` + `MockSsgDockerOps` (SETTLED #37)
- [x] Pure helpers extracted from `handlers/ssg/mod.rs` (`build_stop_response_*`, `build_rm_response_*`, `format_shadow_gate_error`)
- [x] Pure helpers extracted from `handlers/ssg/checkout.rs` (`select_uncheckout_rows`, `build_uncheckout_none_matching_response`)
- [x] `AutoCreateDbDispatch` + `classify_service_for_auto_create_db` in `handlers/run/auto_create_db.rs`
- [x] `build_doctor_response` pure helper in `handlers/ssg/doctor.rs`
- [x] `tarball_path_for` + `pull_step_label` pure helpers in `build/images.rs`

New unit tests (+77 tests relative to Phase 8 baseline of 3205; total 3282):
- [x] `state/ssg.rs` 83% -> 87% (8 new CRUD edge-case tests)
- [x] `handlers/ssg/doctor.rs` 59% -> 89% (5 new `build_doctor_response` tests)
- [x] `handlers/ssg/checkout.rs` 31% -> 58% (10 new pure + AppState-backed tests)
- [x] `handlers/run/auto_create_db.rs` 34% -> 83% (8 new classifier + dispatch tests)
- [x] `handlers/run/ssg_integration.rs` 64% -> 68% (2 new async tests, COAST_HOME-serialized)
- [x] `coast-ssg/src/build/artifact.rs` 82% -> 86% (4 new `auto_prune` error-path tests)
- [x] `coast-ssg/src/build/images.rs` 0% -> 32% (5 new `tarball_path_for` / label tests)
- [x] `coast-ssg/src/docker_ops.rs` new file at 93% (15 tests incl mock-vs-real parity)
- [x] `coast-core/src/protocol/tests.rs` (3 new `SsgLogChunk` round-trip tests)
- [x] `coast-core/src/coastfile/tests_parsing.rs` (1 new `auto_create_db = false` override test)

DESIGN doc reconciliation:
- [x] §6 error wording matches implementation (via §17 SETTLED #30 prefix note + §6 updates)
- [x] §10.3 wording reflects `create_dir_all` default (vs promised UID/GID ownership)
- [x] §13 psql block clarifies `sh -c` vs `\gexec` and cross-refs SETTLED #20
- [x] §17 SETTLED #28-36 added for each divergence or deviation

Verification:
- [x] `make lint` clean (zero warnings, zero `#[allow]` added)
- [x] `make test` — 3282 tests pass, zero failures
- [x] `make run-dind-integration TEST=test_ssg_auto_start_on_run` — passes with updated Negative A + B cases
- [x] `make run-dind-integration TEST=test_ssg_doctor` — regression
- [x] `make run-dind-integration TEST=test_ssg_host_checkout` — regression

### Phase 10 — Cleanup + §17 resolution
Strip every "this is a stub" / "this is future work" / "decide in
Phase X" marker that Phase 9 audit found. See Addendum for context.
- [x] Delete `coast-ssg/src/port_checkout.rs` stub (actual code lives in `runtime/port_checkout.rs` + `handlers/ssg/checkout.rs`); remove `pub mod port_checkout;` from `lib.rs`
- [x] Strip `TODO(ssg-phase-N)` prefixes from `pub mod` comments in `coast-ssg/src/lib.rs`
- [x] Update `# Phase 0 status` doc-comment in `lib.rs` to reflect Phase 10+ reality
- [x] Promote §17 #3 (per-project network) to SETTLED
- [x] Promote §17 #5 (ssg_mutex vs RwLock) to SETTLED
- [x] Promote §17 #8 (checkout displacement `--force`) to SETTLED

### Phase 11 — Consumer socat refresh after SSG re-run
Fixes the "coast ssg rm + run invalidates consumer proxies" gap
found in the Phase 9 post-hoc audit.
- [x] New `coast-daemon/src/handlers/ssg/consumer_refresh.rs::refresh_consumer_proxies_after_lifecycle`
- [x] Call site wiring after `run_streaming_run` + `run_streaming_start_or_restart`
- [x] Integration test `test_ssg_rm_run_refreshes_consumers`
- [x] Unit test using in-memory AppState (Pattern C)
- [x] §17 SETTLED #38 documenting the refresh invariant

### Phase 12 — Pattern A: full lifecycle + images migration
Completes the Pattern A work Phase 9 left partial. Supersedes §17 #37.
- [x] Expand `SsgDockerOps` trait with all lifecycle + image methods
- [x] Complete `BollardSsgDockerOps` real impl
- [x] Complete `MockSsgDockerOps` test impl
- [x] Refactor `runtime/lifecycle.rs` to take `&dyn SsgDockerOps`
- [x] Refactor `build/images.rs::pull_and_cache_ssg_images` to take the trait
- [x] Wire `BollardSsgDockerOps` at daemon entry points
- [x] ~40 new unit tests (proportional to real complexity; parity tests dropped — integration covers real impl)
- [x] Full integration regression: every `test_ssg_*.sh` still passes
- [x] Update §17 SETTLED #37 to document completion

### Phase 13 — `file:/path` inject runtime
Supersedes §17 #20 / #21 (file-inject "deferred follow-up"). DESIGN §14
becomes reality, not half-reality.
- [x] Remove `InjectType::File(_) => continue` early-return in `shared_services.rs`
- [x] Add `shared_service_inject_file_writes` helper
- [x] Compose rewrite mounts the host files into every non-stubbed service
- [x] Integration test `test_ssg_inject_file`
- [x] DESIGN §14 stripped of "deferred" language
- [x] §17 SETTLED #20/#21 updated to "DONE in Phase 13"

### Phase 14 — Phase 9 integration test backfill
Phase 9 fixed 6 user-facing behaviors but only had unit tests. Add
integration coverage.
- [x] `test_ssg_build_global_working_dir`
- [x] `test_ssg_checkout_positional`
- [x] `test_ssg_consumer_disables_auto_create_db`
- [x] `test_ssg_ps_live_state`
- [x] `test_ssg_coast_build_hard_errors_no_ssg`
- [x] `test_ssg_auto_start_on_run` updated to assert event ordering
- [x] Each new test registered in `dindind/integration.yaml`

### Phase 15 — `coast ssg import-host-volume <name>`
Supersedes DESIGN §10.7 "out of scope for v1". Zero-copy migration
of existing host Docker volumes into SSG bind mounts.
- [x] `SsgRequest::ImportHostVolume` protocol variant
- [x] `coast ssg import-host-volume <name>` CLI subcommand
- [x] `coast-ssg/src/runtime/host_volume_import.rs` orchestrator
- [x] `--apply` flag rewrites the SSG Coastfile in place (with backup)
- [x] Docs update in `docs/shared_service_groups/VOLUMES.md`
- [x] Integration test `test_ssg_import_host_volume`
- [x] §17 SETTLED #40 + DESIGN §10.7 updated (slot #39 was taken by Phase 12's fat-trait entry)

### Phase 16 — `coast ssg checkout-build <id>` consumer pinning
Supersedes DESIGN §17-9 "Not in v1". Lets a consumer pin to an older
SSG build so drift doesn't break them.
- [x] New state table `ssg_consumer_pins`
- [x] `SsgRequest::CheckoutBuild` / `UncheckoutBuild` / `ShowPin`
- [x] `coast ssg checkout-build` / `uncheckout-build` / `show-pin` CLI verbs
- [x] `validate_ssg_drift` respects the pin
- [x] `ensure_ready_for_consumer` auto-starts the pinned build
- [x] `auto_prune` is pin-aware (pinned builds are never pruned)
- [x] Docs new page `docs/shared_service_groups/PINNING.md`
- [x] Integration tests: pin-protects, uncheckout-restores-latest, missing-build-errors
- [x] §17 SETTLED #41 + §17-9 promoted

### Phase 17 — `extends` / `includes` on SSG Coastfile
Supersedes §17-7. Mirrors the regular-Coastfile inheritance system.
- [x] `RawSsgCoastfile` accepts `[ssg] extends = ...` and `includes = [...]`
- [x] Parser merges extended/included chains
- [x] Validation: extended file must be a valid SSG Coastfile; no cycles
- [x] `[unset]` supports the SSG's one named collection (`shared_services`); `[omit]` is N/A (no compose file)
- [x] 18 unit tests
- [x] Integration test `test_ssg_coastfile_inheritance`
- [x] `docs/coastfiles/SHARED_SERVICE_GROUPS.md` + DESIGN §5 updated
- [x] §17 SETTLED #42 + §17-7 promoted

### Phase 18 — Symmetric remote shared-service routing
Resolves the inline-vs-SSG collision on shared remotes by mirroring
the local topology (docker0 alias IP + socat inside DinD) on the
remote side. Replaces the Phase 4.5 gateway-IP design in §20. See
§17 SETTLED #43 for rationale.

- [x] Protocol: add `remote_port: u16` to `coast_core::protocol::SharedServicePortForward`
- [x] Lift local shared-service routing helpers out of
      `coast-daemon/src/handlers/shared_service_routing.rs` into a
      shared crate (`coast-docker::shared_service_routing` or new
      `coast-routing`) so both `coast-daemon` and `coast-service`
      call the same primitives
- [x] `coast-daemon::handlers::run::mod::setup_shared_service_tunnels`
      allocates a unique `remote_port` per forward via
      `port_manager::allocate_dynamic_port`, sends it in the RunRequest
- [x] `coast-daemon` reverse-tunnel pairs become
      `(remote_port, local_port)` where `local_port` is the value
      already produced by `rewrite_reverse_tunnel_pairs`
- [x] `coast-service::handlers::run` spawns inside-DinD socat + docker0
      alias IP setup, forwarding canonical -> `host.docker.internal:{remote_port}`
- [x] `coast-service::handlers::run::add_shared_extra_hosts` emits
      `{name}:{alias_ip}` instead of `{name}:{gateway_ip}`
- [x] `RemoteInstance` state tracks per-service `remote_port` for
      lifecycle cleanup (stop/rm/daemon-restart recovery)
- [x] Unit tests: alias-IP allocation inside coast-service, socat script
      generation, `SharedServicePortForward.remote_port` round-trip
- [x] Integration test `test_remote_mixed_inline_and_ssg_no_collision`
      (two coasts on one remote, same canonical port, different upstreams)
- [x] Integration test `test_remote_multi_instance_independent_tunnels`
- [x] `make lint`, `make test`, and full `integrated-examples/remote/test_*.sh` regression green

### Phase 18.5 — Pre-correction disk reclamation (prerequisite for §23)
One-shot host hygiene so Phases 19-25 have room to build, test, and
spin up per-project SSGs. No code changes; runbook captured in
`.cursor/plans/ssg-correction-per-project_*.plan.md`.
- [x] Snapshot `df -h /` and `docker system df` before
- [x] `coast-dev ssg rm --with-data --force` — drop the singleton SSG + inner volumes
- [x] `docker system prune -a --volumes -f` — reclaim dangling images, unused volumes, build cache
- [x] Gate: `df -h /` shows at least 20 GB free before proceeding to Phase 19

### Phase 19 — DESIGN.md correction (no code, just doc)
Documents the singleton-vs-per-project misunderstanding and rewrites
the current-truth contract. Existing §§1-22 stay put as the historical
record; §23 is the new current truth, with banner pointers inserted
at every contradicting claim.
- [x] Append §23 "Post-v1 correction: per-project SSG" at the end of DESIGN.md
- [x] Banner pointers in §§2, 3, 4, 8, 11, 18, 19, 22 and the Addendum's "Out of scope" block citing §23
- [x] TOC entry + Phase 19-25 checklist stubs in §0 (this commit)

### Phase 20 — State schema: per-project SSG
Drops the singleton SQL constraints and keys SSG state by project.
Pre-launch, so existing singleton rows are wiped on daemon startup.
Also elevated the `SsgRequest` protocol type to a `{project, action}`
wrapper (the CLI resolves `project` from the sibling `Coastfile`'s
`[coast].name`) so the per-project key reaches the daemon without a
placeholder constant.
- [x] Replace `id INTEGER PRIMARY KEY CHECK (id = 1)` with `project TEXT PRIMARY KEY` on `ssg`
- [x] Add `project TEXT NOT NULL` to `ssg_services`; PK becomes `(project, service_name)`
- [x] Add `project TEXT NOT NULL` to `ssg_port_checkouts`; PK becomes `(project, canonical_port)`
- [x] Thread `project: &str` through every `SsgStateExt` method in `coast-ssg/src/state.rs`
- [x] One-shot startup migration: `migrate_ssg_per_project()` DROPs + recreates the three SSG tables when the old `id INTEGER` column is detected
- [x] Replace the "CHECK (id = 1) must reject id = 2" test with `two_projects_coexist_under_per_project_schema` + cross-project isolation tests on `ssg_services` and `ssg_port_checkouts`
- [x] Wrap `SsgRequest` in a `{project, action}` struct; the CLI resolves `project` from cwd (`resolve_consumer_project`)
- [x] `cargo build --workspace` green; `cargo test -p coast-daemon -p coast-ssg -p coast-core` green (2161 tests); `cargo clippy --workspace -- -D warnings` clean

### Phase 21 — Naming helpers derive from project
Flips `SSG_CONTAINER_NAME`, `SSG_COMPOSE_PROJECT`, and `INNER_VOLUME_LABEL_FILTER`
from constants to pure functions of `project`. The single-line change
to `DindConfigParams::new("coast", "ssg", ...)` is what makes Docker
Desktop show `{project}-coasts/{project}-ssg` instead of
`coast-coasts/coast-ssg`.
- [x] `fn ssg_container_name(project: &str) -> String` returns `{project}-ssg`
- [x] `fn ssg_compose_project(project: &str) -> String`
- [x] `fn inner_volume_label_filter(project: &str) -> String`
- [x] `DindConfigParams::new("coast", "ssg", ...)` → `DindConfigParams::new(project, "ssg", ...)`
- [x] Update `auto_create_db` `-p "coast-ssg"` call sites to use `ssg_compose_project(project)` (plus the daemon-side `handle_ssg_logs_streaming` literal)
- [x] Thread `project: &str` through `run_ssg` / `run_ssg_with_build_id`; per-record functions (`stop_ssg` / `start_ssg` / `restart_ssg` / `rm_ssg` / `exec_ssg` / `logs_ssg`) use `record.project` directly
- [x] Unit tests: `naming_helpers_derive_from_project`; mock assertions flipped to `{test-proj}-ssg` labels; `build_argv_minimal_psql_command` now takes a project and asserts the derived compose label
- [x] `cargo build --workspace` green; `cargo test -p coast-daemon -p coast-ssg -p coast-core` green (2161 tests); `cargo clippy --workspace -- -D warnings` clean

### Phase 22 — CLI + lifecycle: resolve project from cwd
Every `SsgRequest` variant grows `project: String`. The CLI resolves
the current project from cwd (same path as `coast build/run`). New
`coast ssg ls` lists all SSGs across projects. Most of this landed
incidentally in Phase 20 (protocol wrapper + CLI resolution); Phase
22 adds the cross-project `ls` verb and a dedicated isolation test.
- [x] Add `project: String` to every `SsgRequest` variant in `coast-core/src/protocol/ssg.rs` (Phase 20: promoted to `{ project, action }` wrapper)
- [x] CLI resolution helper reused from `coast build/run` (Phase 20: `resolve_consumer_project` reads `[coast].name` from the sibling Coastfile)
- [x] New `coast ssg ls` subcommand (`SsgAction::Ls` + `SsgListing` + `handle_ls` + `execute_ls` + `format_listings_table`). Cross-project: empty-string `project` sentinel, no cwd Coastfile required.
- [x] Project-scoped error messages everywhere (Phase 20 for all project-scoped paths; build-artifact errors stay global intentionally since the artifact store is global)
- [x] Integration test `test_ssg_per_project_isolation.sh`: two projects each with own `Coastfile.shared_service_groups`, distinct `{project}-ssg` containers run concurrently, `ssg ps` is cwd-scoped, `ssg ls` is cross-project, per-project `rm` is isolated
- [x] `cargo build --workspace` green; `cargo test -p coast-daemon -p coast-ssg -p coast-core` green (2165 tests); `cargo clippy --workspace -- -D warnings` clean; manual `coast-dev ssg ls` smoke passes

### Phase 23 — Consumer Coastfile + `from_group` scoped to own project
Consumer `from_group = true` resolves ONLY against its own project's
SSG. Missing = hard error (never falls back to another project's SSG).
Auto-start logic (§11.1) becomes per-project.
- [x] `Coastfile.shared_service_groups` derives project from sibling `Coastfile`'s `[coast] name`
- [x] Optional explicit `[ssg] project = "..."` (validate match with sibling or error)
- [x] Consumer `from_group = true` resolver looks up only the consumer's own project's SSG
- [x] Hard-error message: `"service 'X' is declared from_group = true in project 'Y' but the SSG Coastfile.shared_service_groups for project 'Y' does not declare it"`
- [x] Delete cross-project sharing paths in `ensure_ready_for_consumer` and equivalents
- [x] Unit + integration tests

### Phase 24 — Remote SSG per-(project, remote)
Thread project through the Phase 18 reverse-tunnel resolution so
remote coasts route to their OWN project's SSG dynamic port, not
a global singleton.
- [x] `shared_service_forwards` project-aware lookup picks the right SSG service port (already keyed by Phase 18; Phase 24 adds explicit multi-project regression tests)
- [x] Per-(project, remote) SSG provisioning on coast-service (coast-service remains SSG-agnostic per §20.1/§20.7; `remote_shared_forwards` already keyed by `(project, instance)`; the LOCAL daemon is where the per-project SSG dynamic port is resolved via `rewrite_reverse_tunnel_pairs`)
- [x] Removed `restored_hosts` host-keyed skip in `restore_shared_service_tunnels` — was a latent cross-project leak on daemon restart under Phase 24's multi-project-on-shared-remote contract
- [x] Regression: `test_remote_multi_instance_independent_tunnels` still green (dedup removal only restores MORE tunnels; no test asserted on the skip)
- [x] New: `test_remote_two_projects_same_canonical_port_distinct_ssg` — two projects (pg15 vs pg16), each own SSG, both consumers on one remote VM, daemon-restart leg exercises dedup fix

### Phase 25 — Integration test sweep
Mechanical updates across 35 tests added between main and
`jrs-add-shared-service-groups` to reflect per-project model.
- [ ] Delete `integrated-examples/test_ssg_port_collision.sh` (premise no longer applies)
- [ ] New `test_ssg_two_projects_same_canonical_port.sh`: two projects, each with own SSG, both using postgres:5432, no collision, each app talks to its OWN postgres
- [ ] New `test_remote_two_projects_same_canonical_port_distinct_ssg.sh`: same idea, one remote VM
- [ ] Update 32 existing SSG tests for container-name renames, project-scoped error messages, and cwd-based project resolution
- [ ] `dindind/integration.yaml` registrations updated
- [ ] `make test` + full `integrated-examples/**/test_*.sh` regression green

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

> **Superseded by §23.** The "singleton DinD" / "one SSG per host"
> framing below is the original (wrong) model. Current truth: one SSG
> per project, named `{project}-ssg`, keyed by the consumer project.
> See §23 for the correction.

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

> **Superseded by §23.** The v1 goals/non-goals below describe the
> singleton-per-host design. Current truth: one SSG per project,
> cross-project sharing removed as a goal, "multiple concurrent SSGs
> on one host" removed as a non-goal. See §23.

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

## 4. High-level architecture

> **Superseded by §23.** The diagram below shows the singleton
> topology with two projects sharing one `coast-ssg` container. Under
> the per-project correction, each project gets its own `{project}-ssg`
> container (e.g. `cg-ssg`, `filemap-ssg`), and each project's
> consumers route to their own project's SSG. See §23.

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
- `[ssg] extends = "..."` and `[ssg] includes = [...]` are supported
  (Phase 17 / SETTLED #42). Merge semantics mirror the regular
  Coastfile: by-name replace for `[shared_services.*]`, child-wins
  scalars for `[ssg]`. Top-level `[unset]` supports
  `shared_services = [...]` to drop inherited entries. Fragments
  cannot themselves use `extends` / `includes`. `[omit]` is N/A for
  SSG (no compose file). See
  [`docs/coastfiles/SHARED_SERVICE_GROUPS.md#inheritance`](../docs/coastfiles/SHARED_SERVICE_GROUPS.md).
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
  exist in the active SSG produces a clear error (the daemon
  emits this at `coast run` time via
  [`missing_ssg_service_error`](./src/daemon_integration.rs)):
  `Consumer references SSG service 'postgres' which does not exist in the active SSG build; available: [...]`
- A block with `from_group = true` plus any forbidden field produces
  (at `coast build` parse time, via
  [`parse_shared_service_group_ref`](../coast-core/src/coastfile/field_parsers.rs)):
  `shared_services.postgres: from_group = true forbids the following fields: image, ports`

Both strings go through `CoastError::coastfile(...)` which adds the
workspace-wide `Coastfile error:` prefix when the CLI renders them
(see §17-30). Integration tests assert on the semantic substring
so the exact wording can evolve without breaking CI.

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

At `coast run`, drift handling is pure-evaluated via
[`coast_ssg::evaluate_drift`](../coast-ssg/src/drift.rs), invoked
from
[`validate_ssg_drift`](../coast-daemon/src/handlers/run/ssg_integration.rs)
on both the local and `--type remote` paths. Always evaluates
against the active SSG's `latest` (not any currently-running SSG —
users who haven't restarted the SSG after building it still see the
drift they introduced):

1. `manifest.ssg.build_id` matches active SSG `latest` -> proceed.
2. Differs but image refs are identical for every referenced service
   -> warn and proceed.
3. Image refs differ or a referenced service is missing -> hard error:
   `"SSG has changed since this coast was built. Re-run `coast build` to pick up the new SSG, or pin the SSG to the old build."`
   Plus a concrete suffix: either `(service '<name>' image changed:
   <old> -> <new>)` or `(service '<name>' is no longer in the active
   SSG; available: [...])`.

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

> **Superseded by §23.** The `CHECK (id = 1)` constraint and the
> flat service / checkout PKs below are the singleton schema.
> Phase 20 replaces `id` with `project TEXT PRIMARY KEY` and adds
> `project` to the composite PK of `ssg_services` and
> `ssg_port_checkouts`. See §23.

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
2. **Pre-run** — for every host bind source,
   [`ensure_host_bind_dirs_exist`](./src/runtime/bind_mounts.rs)
   calls `std::fs::create_dir_all`. Ownership is whatever `coastd`
   inherits (typically the user running the daemon). For images
   like postgres that need specific UID/GID on the data directory
   (UID 999 debian, UID 70 alpine), the user runs `coast ssg
   doctor` to detect mismatches and the `chown` command it prints
   to fix them. We don't auto-`chown` because silently mutating
   ownership on user-owned host bytes is dangerous.
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

Ugly but zero-copy. Phase 15's
[`coast ssg import-host-volume`](../docs/shared_service_groups/VOLUMES.md#automating-the-recipe-coast-ssg-import-host-volume)
command automates this: it resolves the host volume's `Mountpoint`
via bollard's `inspect_volume`, validates the target service exists
in the SSG Coastfile, rejects duplicate mount paths, and either
prints the TOML snippet (default) or rewrites the Coastfile in
place with a `.bak` backup (`--apply`). See §17-40 for the full
set of design decisions.

## 11. Port plumbing (coast -> SSG)

> **Superseded by §23.** The routing chain described below is
> correct, but step 2 ("looks up `(container_port, dynamic_host_port)`
> in `ssg_services`") becomes project-scoped under per-project SSGs:
> the daemon resolves the consumer's own project's SSG services only.
> Cross-project routing is explicitly rejected. See §23.

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
- After every `run` / `start` / `restart` commits new
  `ssg_services.dynamic_host_port` rows, the daemon refreshes each
  local running consumer's in-dind socat forwarders so existing
  consumer coasts survive an SSG port reallocation without rebuilding.
  See §17-38.

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
  checked out on canonical 5432 (because its own Coastfile declared a
  service on that port and the user ran `coast checkout <instance>`),
  the SSG checkout displaces it:
  1. Look up the current holder via
     [`StateDb::find_port_allocation_holding_canonical`](../coast-daemon/src/state/ports.rs)
     — any `port_allocations` row with `socat_pid NOT NULL` on that
     canonical port.
  2. Kill the existing socat PID.
  3. Clear `port_allocations.socat_pid` for that holder. (The
     `is_primary` column is orthogonal — it's the "primary service
     for display" tag, not the "owns the canonical port" claim.)
  4. Spawn the SSG-owned socat.
  5. Record the SSG as the new owner in `ssg_port_checkouts`.
- **Only coast-known holders are displaced.** If the canonical port
  is held by a process outside Coast's tracking (a stray `postgres`
  the user started by hand, another daemon, etc.),
  `coast ssg checkout` errors out and the user must free the port
  themselves. See §17-23.
- Displacement is visible: CLI prints a warning listing what was
  displaced; Coastguard emits a structured event. No `--force` flag
  is required, but the message is unambiguous.
- The displaced coast instance is NOT auto-rebound when the SSG
  uncheckouts. It stays on dynamic-only until the user runs
  `coast checkout <instance>` again.
- Coasts **always** reach SSG services via the docker0 alias-IP
  path, regardless of host-side checkout state. Checkout is purely a
  host-side convenience.
- **Stop/start semantics**: `coast ssg stop` kills the checkout
  socats and nulls their `socat_pid`, but preserves the
  `ssg_port_checkouts` rows. `coast ssg run` / `start` re-spawn
  against the newly-allocated dynamic host ports. `coast ssg rm`
  wipes the rows (destructive). See §17-22.

## 13. `auto_create_db`

Phase 5 lights up `auto_create_db` for both paths (inline and SSG)
— prior to that, the SQL builder existed but had no caller (§17-20).
The pseudocode below shows the conceptual shape; the actual
implementation wraps the `psql` call in `sh -c` with a
`SELECT 'CREATE DATABASE ...' WHERE NOT EXISTS` guard instead of
`\gexec`, because `\gexec` is a psql meta-command that cannot be
composed with `-c` (see §17-20 for the settled rationale).

```text
# Inline (host Docker daemon):
docker exec <host-container> psql -U postgres -c "..."

# SSG (nested):
docker exec <ssg-outer> \
  docker compose exec -T <inner-postgres-service> \
  psql -U postgres -c "..."
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

Both `env:NAME` and `file:/path` are fully wired for inline and
SSG-backed shared services:

- **`env:NAME`** — via
  [`coast-daemon/src/shared_services.rs::shared_service_inject_env_vars`].
  Sets `$NAME` in every non-stubbed inner compose service.
- **`file:/path`** — via
  [`coast-daemon/src/shared_services.rs::shared_service_inject_file_writes`].
  Writes the same canonical connection URL bytes inside the
  consumer's coast DinD (through the secret-file exec path at
  [`handlers/run/secrets.rs::write_secret_files_via_exec`](../coast-daemon/src/handlers/run/secrets.rs))
  and bind-mounts `{path}:{path}:ro` into every non-stubbed inner
  compose service via
  [`handlers/run/compose_rewrite.rs::ensure_secret_mounts`](../coast-daemon/src/handlers/run/compose_rewrite.rs).
  The host file body is byte-identical to what `env:NAME` would
  have set, so the two variants are interchangeable for consumers.
  Non-absolute container paths are rejected at provision time; path
  collisions with existing secrets are hard errors. See §17-20 /
  §17-21 for the full settlement.

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
3. (SETTLED — Phase 10) **Per-project network kept.** `coast-shared-{project}`
   networks are still created and coasts are still attached, even though
   SSG routing uses `host.docker.internal` via docker0 alias IPs and
   does not strictly require the bridge. Creation is cheap, keeps the
   compose rewriter uniform across inline and `from_group` services,
   and leaves users free to attach ad-hoc containers to the same
   project bridge. No issues surfaced across Phases 2-9 or the Phase 9
   post-hoc audit. See
   [`coast-docker/src/network.rs`](../coast-docker/src/network.rs)
   (`shared_network_name`, `create_shared_network`).
4. (SETTLED) Auto-start SSG from `coast run` (hard error only if no
   SSG build exists).
5. (SETTLED — Phase 3) **`Mutex<()>` kept; no `RwLock` migration.**
   Every SSG-mutating handler acquires a process-global
   `ssg_mutex: tokio::sync::Mutex<()>` in `AppState`. Coast-run paths
   that auto-start the SSG check state first and only acquire the
   lock when actually starting. Read-only verbs (`ps`, `ports`,
   `logs`, `exec`) do not take the mutex at all, so the theoretical
   `RwLock` win (many concurrent readers) has no practical payoff —
   readers already run lock-free. An `RwLock` would only add
   complexity and a footgun (readers accidentally held across
   mutating awaits). See
   [`coast-daemon/src/server.rs`](../coast-daemon/src/server.rs)
   (`pub ssg_mutex: Mutex<()>` around line 221, and the
   `_ssg_guard = state.ssg_mutex.lock().await` pattern in
   `handle_ssg_lifecycle_streaming`).
6. (SETTLED) Remote coasts use the existing
   `SharedServicePortForward` protocol (§20). `coast-service` is
   unchanged. Remote SSG is an explicit non-goal for v1.
7. (SETTLED — Phase 17) **Coastfile.shared_service_groups
   inheritance** — `[ssg] extends = "..."`, `[ssg] includes = [...]`,
   and top-level `[unset] shared_services = [...]` are wired.
   Fragments cannot themselves use `extends` / `includes`;
   `[omit]` is N/A (no compose file in the SSG); cycles
   hard-error. `build_id` hashes the flattened
   [`to_standalone_toml`](./src/coastfile/mod.rs) output so
   parent-only changes invalidate the cache correctly. See
   [`docs/coastfiles/SHARED_SERVICE_GROUPS.md#inheritance`](../docs/coastfiles/SHARED_SERVICE_GROUPS.md)
   and SETTLED #42 for the 8 design decisions.
8. (SETTLED — Phase 6) **No `--force` flag on `coast ssg checkout`.**
   Coast-tracked holders (a coast-instance's canonical-port socat, or
   an existing `ssg_port_checkouts` row) are displaced cleanly with a
   clear CLI warning. Unknown-process holders yield
   `unknown_holder_error` and instruct the user to free the port
   manually — silently killing an unknown process was judged too
   dangerous. See SETTLED #23 for the full rules and the
   [`unknown_holder_error`](../coast-daemon/src/handlers/ssg/checkout.rs)
   call site.
9. (SETTLED — Phase 16) **Pinning a consumer coast to an older SSG
   build** — `coast ssg checkout-build <BUILD_ID>` now exists.
   Consumer-local pins keyed by project name override the
   `~/.coast/ssg/latest` symlink for drift evaluation and auto-start;
   `auto_prune` preserves every pinned build across rebuilds; a
   pruned pin hard-errors with remediation guidance. See
   [`coast-ssg/src/runtime/pinning.rs`](src/runtime/pinning.rs),
   [`coast-daemon/src/handlers/ssg/pin.rs`](../coast-daemon/src/handlers/ssg/pin.rs),
   [`coast-ssg/src/build/artifact.rs::auto_prune_preserving`](src/build/artifact.rs),
   `docs/shared_service_groups/PINNING.md`, and SETTLED #41 for the
   six design decisions.
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
25. (SETTLED — Phase 7) **Warn wording for same-image drift is
    daemon-shaped, not in DESIGN.** DESIGN.md §6.1 pins the
    hard-error sentence verbatim but leaves the warn case
    unspecified. The daemon emits:
    `"SSG build differs (was <old>, now <new>) but image refs still
    match for every referenced service. Proceeding."` — via a
    `BuildProgressEvent::done("Checking SSG drift", <detail>)`
    with status set to the detail string so the CLI renders it
    inline. See
    [`validate_ssg_drift`](../coast-daemon/src/handlers/run/ssg_integration.rs).
24. (SETTLED — Phase 7) **`coast-service` stays SSG-free; the
    `ssg` drift block is injected locally post-download.**
    DESIGN.md §6.1 says `coast build` records an `ssg` block in
    the consumer's `manifest.json`. For local builds this is
    straightforward — [`build::manifest::write_manifest_and_finalize`](../coast-daemon/src/handlers/build/manifest.rs)
    computes the block via `build_ssg_manifest_block`. For
    `coast build --type remote`, the actual build runs under
    `coast-service`, which is SSG-agnostic by design (see §4.1,
    §17-10). Rather than breaking that invariant, the local
    daemon patches the block onto the downloaded manifest in
    [`download_remote_artifact`](../coast-daemon/src/handlers/run/mod.rs)
    via `phase7_ssg_block_for_artifact`. Users see identical
    drift behavior on both paths.
23. (SETTLED — Phase 6) **`coast ssg checkout` displaces only
    coast-tracked holders; unknown-process strangers yield an
    error.** If the canonical port is held by a `port_allocations`
    row with `socat_pid NOT NULL` (i.e. a coast currently checked
    out) or an existing `ssg_port_checkouts` row, the SSG checkout
    kills that socat, clears its DB state, and binds. Any other
    holder — a host postgres the user started manually, an unrelated
    development daemon, `nginx` on 8080, etc. — causes
    `coast ssg checkout` to return
    [`unknown_holder_error`](../coast-daemon/src/handlers/ssg/checkout.rs)
    with a message instructing the user to free the port first. No
    `--force` flag: silently killing an unknown process was judged
    too dangerous, and the remediation is one command away. DESIGN
    §12 and §17-8.
22. (SETTLED — Phase 6) **`coast ssg stop` preserves
    `ssg_port_checkouts` rows; `run / start` re-spawns them.** Stop
    kills the checkout socats (they'd be forwarding to a
    now-dead dynamic port anyway) and sets `socat_pid = NULL` on
    each row. The next `coast ssg run` or `coast ssg start` iterates
    the rows and spawns a fresh socat per row against the newly-
    allocated dynamic port via
    [`respawn_checkouts_after_lifecycle`](../coast-daemon/src/handlers/ssg/checkout.rs).
    Rows whose service has disappeared from the new build are
    dropped with a warning message in the response. `coast ssg rm`
    clears the rows entirely (destructive — the user explicitly
    asked for a clean slate). Daemon restart (outside of stop) goes
    through the same re-spawn helper from `restore_running_state`.
    Rationale: users pay-once-for-checkout across routine
    stop/start cycles without losing stale state that pointed at a
    port that no longer matches the new dynamic allocation.
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
20. (SETTLED — Phase 5 / Phase 13) **Inline `auto_create_db` was not
    actually implemented before Phase 5, despite §13 claiming
    otherwise.**
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
    `file:/path` inject was deferred in Phase 5 and is **fully
    implemented in Phase 13** via
    [`shared_service_inject_file_writes`](../coast-daemon/src/shared_services.rs):
    the URL bytes go through
    [`handlers/run/secrets.rs::write_secret_files_via_exec`](../coast-daemon/src/handlers/run/secrets.rs)
    into the consumer DinD at the declared path and then get
    bind-mounted into every non-stubbed inner compose service via
    the existing
    [`ensure_secret_mounts`](../coast-daemon/src/handlers/run/compose_rewrite.rs)
    machinery. Integration coverage:
    `test_ssg_auto_create_db`, `test_ssg_inject_env`,
    `test_ssg_inject_file`, `test_shared_service_auto_create_db`.
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
26. (SETTLED — Phase 8) **SSG concept docs live in a top-level
    `docs/shared_service_groups/` folder, not under
    `concepts_and_terminology/`.** The original Phase 8 checklist
    called for a single `docs/concepts_and_terminology/SHARED_SERVICE_GROUPS.md`.
    User feedback requested a dedicated top-level section mirroring
    [`docs/remote_coasts/`](../docs/remote_coasts/README.md) because
    the feature is multi-facet enough (build + lifecycle + volumes +
    consumer + checkout + CLI) that one page gets unwieldy. Phase 8
    therefore landed seven pages (README + BUILDING + LIFECYCLE +
    VOLUMES + CONSUMING + CHECKOUT + CLI) under the new folder,
    with cross-links back from `concepts_and_terminology/SHARED_SERVICES.md`
    so existing readers find their way in. The new section is
    inserted between "Coastfiles" and "Remote Coasts" in
    [`docs/doc_ordering.txt`](../docs/doc_ordering.txt) and wired
    through
    [`coast-guard/src/components/DocsSidebar.tsx`](../coast-guard/src/components/DocsSidebar.tsx)
    plus [`coast-guard/src/locales/en.json`](../coast-guard/src/locales/en.json).
    Non-English locales are intentionally untouched — the canonical
    `make translate-all` flow owns those.
27. (SETTLED — Phase 8) **`coast ssg doctor` is read-only and only
    warns on the upstream-official image set.** The doctor consults
    a built-in `KnownImage` table covering `postgres`, `mysql`,
    `mariadb`, and `mongo` (with per-tag UID/GID disambiguation: the
    alpine-tagged `postgres:*-alpine` expects 70:70, the debian
    default expects 999:999). Services whose image does not match
    any prefix are silently skipped — forks like
    `ghcr.io/baosystems/postgis` would otherwise produce false
    positives and erode trust in the tool. The command never calls
    `chown` itself; the `warn` finding instead emits the exact
    `sudo chown -R {uid}:{gid} {path}` command the user should run.
    Evaluation is a pure function
    ([`coast_ssg::doctor::evaluate_doctor`](./src/doctor.rs))
    with a stat closure injected by the daemon adapter
    ([`coast-daemon/src/handlers/ssg/doctor.rs`](../coast-daemon/src/handlers/ssg/doctor.rs)),
    so unit tests cover every match/mismatch/missing path without
    touching the real filesystem. Integration coverage:
    [`test_ssg_doctor.sh`](../integrated-examples/test_ssg_doctor.sh)
    exercises the ok/warn/info paths plus a read-only regression
    (bind-mount mtime unchanged across invocations).
28. (SETTLED — Phase 9) **Protocol extensions beyond the §7 sketch.**
    §7 shows the Phase-1 shape of `SsgRequest` / `SsgResponse`.
    Subsequent phases extended them without rewriting §7. The
    normative shape at HEAD is
    [`coast-core/src/protocol/ssg.rs`](../coast-core/src/protocol/ssg.rs);
    additions are:
    - `SsgRequest::Doctor` (Phase 8, §10.5 / SETTLED #27)
    - `SsgRequest::Stop { force }` (Phase 4.5, §20.6)
    - `SsgRequest::Rm { with_data, force }` (Phase 4.5, §20.6)
    - `SsgResponse::findings: Vec<SsgDoctorFinding>` (Phase 8)
    - `SsgDoctorFinding` struct (Phase 8)
    All new variants / fields use `#[serde(default)]` so older
    clients round-trip through without breakage.
29. (SETTLED — Phase 9) **`env` TOML map coerces non-string scalars.**
    §5 says "flat string map". The parser
    ([`coerce_env_value`](./src/coastfile/mod.rs)) accepts TOML
    integers, floats, and booleans and turns them into strings
    (`42` -> `"42"`, `true` -> `"true"`). This keeps round-trips
    with `Coastfile.shared_service_groups` files written by humans
    (who often forget the quotes around numeric env values like
    `MAX_CONNECTIONS = 200`) symmetric with what `docker compose`
    would have done. Strict string-only would have broken everyday
    ergonomics. The runtime still sees proper strings because we
    coerce at parse time, not at serialization.
30. (SETTLED — Phase 9) **`Coastfile error:` prefix on coastfile-
    class errors is a workspace-wide convention.**
    [`coast-core/src/error.rs`](../coast-core/src/error.rs) wraps
    every `CoastError::coastfile(...)` message with the literal
    `Coastfile error:` prefix before emitting. §6 shows
    `error: shared service ...` for brevity but the actual wire
    form is `Coastfile error: shared service ...`. Integration
    tests assert on the semantic substring (e.g. `"references the
    shared service group"`) rather than the verbatim DESIGN
    sentence. Same pattern applies to every
    `CoastError::docker(...)`, `::port(...)`, etc. — each carries
    a class prefix so the user can see at a glance whether the
    failure is a config parse, a Docker call, a port bind, etc.
31. (SETTLED — Phase 9) **Global `--working-dir` flows into
    `coast ssg build`.** DESIGN.md §5 promised
    `coast --working-dir <dir> ssg build` would work, but the
    original dispatch in
    [`coast-cli/src/lib.rs`](../coast-cli/src/lib.rs) routed
    `Ssg` through `dispatch_project_command` (which resolves a
    project from cwd) and dropped `cli.working_dir` on the floor.
    Phase 9 moves `Ssg` into the top-level `dispatch()` as a
    singleton/project-less command and forwards `&cli.working_dir`
    to `ssg::execute`. The `--working-dir` flag on `Cli` is
    `global = true`, so clap also threads the value into the
    subcommand's own `working_dir` field when it's set before
    `ssg build`; `execute_build` additionally ORs the two as a
    belt-and-braces defense against future clap changes.
32. (SETTLED — Phase 9) **`coast ssg checkout` / `uncheckout` accept
    both positional and `--service` forms.** DESIGN.md §12 uses the
    positional form (`coast ssg checkout postgres`). The first
    implementation only shipped `--service` because of a clap derive
    quirk (can't put `#[arg(long)]` and positional on the same field).
    Phase 9 keeps the existing `--service` flag working for backward
    compat and adds a positional `SERVICE` on a parallel field;
    `resolve_checkout_service` merges them at dispatch time, rejects
    conflicting values, and rejects `--all + SERVICE` combinations.
    Both forms produce the same `SsgRequest::Checkout { service, all }`
    on the wire.
33. (SETTLED — Phase 9) **`coast build` hard-errors at build time
    when `from_group = true` refs exist but no SSG build exists.**
    The original implementation returned `None` from
    [`build_ssg_manifest_block`](../coast-daemon/src/handlers/build/manifest.rs)
    silently when no SSG build existed, which weakened drift
    detection — consumers could build without the `ssg` block and
    sneak past the §6.1 drift check at run time. Phase 9 hard-errors
    at `coast build` so the consumer's manifest always carries the
    block when refs exist. The run-time §11.1 error now acts as a
    belt for the narrow case where the SSG was removed between
    `coast build` and `coast run`.
34. (SETTLED — Phase 9 / Phase 14) **Consumer `auto_create_db = false`
    is an explicit disable override.** Originally
    [`parse_shared_service_group_ref`](../coast-core/src/coastfile/field_parsers.rs)
    mapped the serde-defaulted `bool` through `if raw { Some(true) }
    else { None }`, which meant the consumer could only force-enable,
    never force-disable. DESIGN.md §6 explicitly allows both. Phase 9
    changed `RawSharedServiceConfig::auto_create_db` to
    `Option<bool>` so `Some(false)` round-trips correctly at the
    parser layer; `synthesize_shared_service_configs` already did
    `consumer_ref.auto_create_db.unwrap_or(manifest_svc.auto_create_db)`.
    **Phase 14 closed a round-trip gap in
    [`coast-core/src/coastfile/serializer.rs`](../coast-core/src/coastfile/serializer.rs):**
    the artifact serializer was emitting `auto_create_db = true`
    only when the value was `Some(true)`, silently dropping
    `Some(false)`. After re-parse the field read back as `None` and
    fell through to the SSG default (usually `true`). The serializer
    now emits `auto_create_db = {true|false}` whenever `Some(_)` and
    omits the line only for `None` (inherit), so the three-valued
    override survives build → artifact → run. Regression test:
    `integrated-examples/test_ssg_consumer_disables_auto_create_db.sh`.
35. (SETTLED — Phase 9 / Phase 14 test) **`SsgStarting` fires before
    the dispatch; `SsgStarted` fires after.** Originally both events
    were emitted together after `run_and_apply` / `start_and_apply`
    completed, which made `Starting` meaningless. Phase 9 emits
    `Starting` before the auto-start work begins (with the existing
    SSG's `build_id` as a best-effort identifier, or `"pending"`
    when we are about to create a fresh one), and `Started` after.
    Both still fire unconditionally on every successful auto-start
    (including the already-running short-circuit) so subscribers
    can rely on the pair as a standard handshake (§17-16).
    `SsgStarting` / `SsgStarted` are daemon-bus events (not
    rendered on CLI); Phase 14's `test_ssg_auto_start_on_run` test
    asserts the CLI-observable handshake instead — the outer
    `Ensure SSG ready` progress line appears before the first
    `SSG:` prefixed inner event in `coast run` output.
36. (SETTLED — Phase 9) **`coast ssg ps` merges live state from
    `ssg_services` + the SSG container record.** §7's table says
    `ps` should show "SSG status + inner service statuses + dynamic
    host ports". The original implementation only read the artifact
    manifest and reported `dynamic_host_port: 0` / `status:
    "built"`. Phase 9 extends
    [`ps_ssg`](./src/daemon_integration.rs) to optionally take a
    `&dyn SsgStateExt` so the daemon can merge runtime rows. When
    no state is available (e.g., a future non-daemon caller) the
    function still falls back to the manifest-only view, preserving
    the pre-Phase-9 contract for compat.
37. (SETTLED — Phase 9 / Phase 12) **`SsgDockerOps` trait is the
    sole Docker seam in the SSG crate.** Phase 9 introduced the
    trait + `BollardSsgDockerOps` real impl + `MockSsgDockerOps`
    test double in [`coast-ssg/src/docker_ops.rs`](./src/docker_ops.rs)
    alongside concrete `&bollard::Docker` + `DindRuntime` usage.
    Phase 12 completed the migration: every SSG async lifecycle +
    image code path
    ([`runtime::lifecycle`](./src/runtime/lifecycle.rs),
    [`build::images`](./src/build/images.rs),
    [`runtime::auto_create_db`](./src/runtime/auto_create_db.rs),
    [`daemon_integration`](./src/daemon_integration.rs)) now goes
    through `&dyn SsgDockerOps`. `DindRuntime` and bollard's
    `Docker` are isolated to `docker_ops.rs`'s real impl; lifecycle
    orchestrators are unit-testable without Docker. `coast-daemon`
    wraps once per entry point via
    `BollardSsgDockerOps::new(docker.clone())` in
    [`handlers/ssg/mod.rs`](../coast-daemon/src/handlers/ssg/mod.rs),
    [`server.rs`](../coast-daemon/src/server.rs), and
    [`handlers/run/{auto_create_db,ssg_integration}.rs`](../coast-daemon/src/handlers/run).
    Pure argv builders (`build_inner_compose_{up,down,exec,logs}_argv`,
    `build_host_docker_logs_argv`) are co-located with the trait so
    the real impl and any future custom impl share the canonical
    command shape. Invariant enforced in CI via grep: zero
    `&bollard::Docker` or `DindRuntime` references remain in
    `coast-ssg/src/runtime`, `coast-ssg/src/build`, or
    `coast-ssg/src/daemon_integration.rs` outside docs.
38. (SETTLED — Phase 11) **Consumer socat refresh invariant.** Every
    SSG lifecycle verb that can mutate
    `ssg_services.dynamic_host_port` (`run` / `start` / `restart`)
    triggers a best-effort refresh of every local running consumer
    coast that references the SSG via `from_group = true`. The
    refresh re-plans socat routing with the current `ssg_services`
    rows and re-runs
    [`ensure_shared_service_proxies`](../coast-daemon/src/handlers/shared_service_routing.rs)
    inside the consumer dind; the proxy script tears down stale PIDs
    before spawning new ones, so the hook is idempotent (no-op when
    dynamic ports did not actually change). Remote shadow coasts
    (`remote_host.is_some()`) are skipped — they re-establish via
    the reverse-tunnel path on their own run cycle. `stop` and `rm`
    do NOT refresh: there is nothing to point proxies at. Per-consumer
    failures are logged and skipped; the lifecycle verb itself never
    fails on refresh errors. Refresh results are announced in the
    lifecycle response message as
    `Refreshed shared-service proxies for: <project>/<instance>, …`.
    See
    [`coast-daemon/src/handlers/ssg/consumer_refresh.rs`](../coast-daemon/src/handlers/ssg/consumer_refresh.rs).
39. (SETTLED — Phase 12) **Fat `SsgDockerOps` trait: one named
    method per semantic operation.** The Phase 12 migration picked
    the fat-trait shape over a leaner alternative that would have
    kept `exec_in_container` as the generic primitive for every
    inner-compose / inner-docker op. The fat trait has 15 methods:
    outer-DinD lifecycle (`create_container`, `start_container`,
    `stop_container`, `remove_container`, `wait_for_inner_daemon`,
    `exec_in_container`), image ops (`pull_and_cache_image`,
    `load_images_into_inner`, `list_inner_images`), inner-volume
    ops (`remove_inner_volumes`), inner-compose ops
    (`inner_compose_up`, `inner_compose_down`, `inner_compose_exec`,
    `inner_compose_logs`), and host-level ops (`host_container_logs`).
    Rationale: mock records stay semantic (tests assert "start
    compose" rather than parsing the canonical `docker compose -f
    …` argv), lifecycle orchestrators stay short, and the real
    impl keeps its thin delegation shape.
    [`MockSsgDockerOps`](./src/docker_ops.rs) exposes per-method
    `push_*_result` setters and a `MockCall` variant per method.
    The mock is also gated behind a `test-support` feature so
    `coast-daemon`'s unit tests can construct one without
    duplicating the code.
40. (SETTLED — Phase 15) **`coast ssg import-host-volume` shape.**
    Phase 15 wires the `SsgRequest::ImportHostVolume` verb and the
    [`coast_ssg::runtime::host_volume_import::run_import`](./src/runtime/host_volume_import.rs)
    orchestrator. Design decisions locked in:
    - **`bollard::Docker::inspect_volume`, not a subprocess.** The
      daemon already holds a `bollard::Docker` handle via
      `AppState.docker`; shelling out `docker volume inspect` would
      add a parsing layer and a new failure mode. The mountpoint
      string comes straight off the `Volume` struct.
    - **`--apply` reuses [`SsgCoastfile::to_standalone_toml`].** The
      parser+serializer round-trip is already verified by existing
      Coastfile tests; no surgical in-place TOML editor is needed.
      The `.bak` backup holds the **original bytes verbatim** so
      users can recover comments and formatting if they want to.
    - **Explicit `--service` + `--mount`, no inference.** A volume
      called `pg_data` targets a container path inside whatever
      service the user says. Guessing ("pg_data" -> "postgres") is
      fragile enough that the error surface outweighs the
      convenience.
    - **Inline `--config` + `--apply` is rejected at validation
      time.** There is no file to write back to; emitting the
      snippet is still possible.
    - **Duplicate `--mount` on the same service is a hard error.**
      Silently merging a second entry targeting the same
      container_path would let a single service declare
      conflicting binds with no warning.
    - **Handler runs through the standard non-streaming SSG
      dispatch** (`handlers/ssg/mod.rs::handle`). The verb is
      analytics-tagged as `ssg/import-host-volume`.
    Regression test: `integrated-examples/test_ssg_import_host_volume.sh`.

41. (SETTLED — Phase 16) **Consumer pinning shape
    (`coast ssg checkout-build`).** Phase 16 wires
    `ssg_consumer_pins` and the
    [`coast_ssg::runtime::pinning`](./src/runtime/pinning.rs)
    orchestrator, with drift + auto-start + auto-prune all honoring
    the pin. Design decisions locked in:
    - **Pins are keyed by project name** (from `RunRequest.project`),
      `PRIMARY KEY (project)`. Multiple worktrees of the same project
      share one pin. Per-worktree scoping was rejected as
      over-engineering for v1; the concurrent-run case is rare enough
      that the simpler schema wins.
    - **`checkout-build` validates the build dir exists on disk at
      pin time** via
      [`validate_pinnable_build`](./src/runtime/pinning.rs).
      Typos and pruned ids fail fast with a message pointing at
      `ls ~/.coast/ssg/builds/`.
    - **`auto_prune` is pin-aware** via
      [`auto_prune_preserving`](./src/build/artifact.rs). The daemon
      loads `ssg_consumer_pins` before the async build, passes the
      set of pinned build ids through `build_ssg`, and pruning skips
      any matching entry. Pinned builds survive SSG churn.
    - **Running SSG + mismatched pin → drift hard-error.** Auto-swap
      would disrupt other consumers sharing the singleton. The user
      resolves explicitly with `coast ssg stop` → `coast run`
      (auto-start boots the pin) or `uncheckout-build` (fall back to
      latest).
    - **Auto-start dispatches on the pinned build id** via
      [`run_ssg_with_build_id`](./src/runtime/lifecycle.rs) rather
      than the `latest` symlink. `run_ssg` keeps its existing
      signature and delegates with `None` so all other callers are
      unchanged.
    - **No `list-pins` in v1.** `show-pin` scopes to the current
      project (resolved from the consumer's Coastfile or `--project`).
      Multi-project listings are low-signal for a local dev tool.
    Regression tests: `test_ssg_pin_protects_from_drift`,
    `test_ssg_uncheckout_build_restores_latest`,
    `test_ssg_pin_missing_build_errors`. Docs:
    `docs/shared_service_groups/PINNING.md`.

42. (SETTLED — Phase 17) **SSG Coastfile inheritance
    (`extends` / `includes` / `[unset]`).** Phase 17 mirrors the
    regular-Coastfile inheritance system on
    `Coastfile.shared_service_groups`. Implementation:
    [`coast_ssg::coastfile::SsgCoastfile::from_file_with_ancestry`](./src/coastfile/mod.rs),
    `merge_raw_onto`, `apply_unset`, `find_ssg_coastfile`. Design
    decisions locked in:
    - **Duplicated the pattern inside `coast-ssg`** rather than
      extract helpers from `coast_core::coastfile`. SSG schema is
      narrow (2 sections); duplication is ~200 lines and avoids
      promoting `pub(super)` internals across crates. Cross-crate
      reuse already exists for `interpolate_env_vars`; no
      additional shared code was needed.
    - **Merge at the raw-TOML level, then validate once.** The
      regular Coastfile merges validated `Coastfile` objects; the
      SSG merges `RawSsgCoastfile` and calls `validate_and_build`
      once on the merged result. Simpler for the narrow schema and
      avoids duplicating validation per layer.
    - **`[unset]` scoped to `shared_services` only.** It's the
      only named collection in the SSG schema. `runtime` is a
      scalar (child-wins handles override). `[omit]` is N/A —
      the SSG has no compose file to strip services from.
    - **Fragments (`includes` targets) cannot themselves use
      `extends` / `includes`.** Matches regular Coastfile. Keeps
      the dependency graph a tree rooted at a single `from_file`
      call; `includes_cannot_have_extends_in_fragment` /
      `includes_cannot_have_includes_in_fragment` tests enforce.
    - **`.toml` tie-break on `extends`**, exact filename on
      `includes`. Matches regular Coastfile. Path resolution is
      relative to the extending/including file's parent directory.
      No `.git` worktree traversal (SSG is host-level; its
      Coastfiles don't live inside worktrees the way regular ones
      do).
    - **`SsgCoastfile::parse(content, _)` rejects
      `extends` / `includes`** with
      `"extends and includes require file-based parsing. Use
      SsgCoastfile::from_file() instead."` — inheritance needs
      disk access to resolve paths. Inline `coast ssg build
      --config '<toml>'` hits this rejection naturally.
    - **`build_id` hashes the flattened
      `SsgCoastfile::to_standalone_toml()`** output rather than
      the raw top-level bytes. Correctness fix: a parent-only
      change now invalidates the build cache, and the artifact
      already stores the same flattened form on disk at
      `~/.coast/ssg/builds/<id>/ssg-coastfile.toml`. Unit test:
      [`coastfile_hash_is_stable_across_equivalent_formatting`](./src/build/artifact.rs).
    - **Artifact stores the flattened standalone TOML.**
      `to_standalone_toml` never emits `extends`, `includes`, or
      `[unset]`, so build artifacts remain self-contained and
      `coast ssg run` needs no parent files to resolve. Matches
      regular Coastfile artifact behavior.
    Regression test: `test_ssg_coastfile_inheritance`. Docs:
    [`docs/coastfiles/SHARED_SERVICE_GROUPS.md#inheritance`](../docs/coastfiles/SHARED_SERVICE_GROUPS.md).

43. (SETTLED — Phase 18) **Inline-vs-SSG collision on shared remotes.**
    The pre-Phase-18 design bound the reverse-tunnel remote port to the
    canonical port. Two consumer coasts on one remote declaring the
    same canonical port (e.g. both `postgres:5432`) with different
    local upstreams (one inline, one SSG) collided: the second
    `reverse_forward_ports` call failed at bind time, and the
    `Err(_) => info!("already bound, reusing existing")` arm in
    [`coast-daemon/src/handlers/run/mod.rs`](../coast-daemon/src/handlers/run/mod.rs)
    silently treated the pre-existing tunnel as fungible, so the
    second coast's traffic reached the first coast's upstream.
    Alternatives considered:
    - **Error-instead-of-reuse when upstreams differ.** Minimal (~50 LOC),
      enforces a constraint on users rather than fixing it.
    - **One-coast-per-VM.** Sidesteps at the cost of cloud spend and
      loses the multi-instance-per-remote property.
    - **Loopback-alias remote binds** (`-R 127.0.0.2:5432:...`).
      sshd supports client-specified binds, but the inner container
      needs per-coast `extra_hosts` addresses the VM has to provision,
      and those loopback aliases aren't routable from the DinD's own
      netns without bridge-side plumbing. More machinery, less symmetric.
    - **Symmetric topology (chosen).** Remote port becomes dynamic per
      forward; socat inside the remote DinD does canonical<->dynamic
      translation exactly like the local path (§11 /
      [`shared_service_routing.rs`](../coast-daemon/src/handlers/shared_service_routing.rs)).
      Collisions impossible because every coast owns its own remote
      port. Minor cost: lifts the shared-routing helpers into a common
      crate and adds socat inside DinD on the remote. See §20 for the
      full specification.

## 18. Risks

- **Hidden port collisions.** Dynamic ports can clash with other
  processes that grab the port between allocation and `docker run`.
  Mitigated by the existing `allocate_dynamic_port` helper which
  already handles retries; SSG reuses it. Cross-coast canonical-port
  collisions on a shared remote VM are impossible by construction
  under the Phase 18 symmetric-routing design (every reverse tunnel
  binds its own ephemeral `remote_port`); see §20 and §17 SETTLED #43.
- **DinD-in-DinD confusion.** Coasts are DinD; the SSG is DinD. They
  are *siblings* on the host Docker daemon, not nested. Coasts reach
  the SSG via `host.docker.internal`, never by nesting. Add a
  paragraph to user docs so this is clear.
- **Docker Desktop UX regression.** Users lose the per-project compose
  view for databases. Mitigation: label the SSG's inner compose
  project as `coast-ssg` consistently, and document that inner
  services are visible via `coast ssg ps`.
  > **Superseded by §23.** The "mitigation" here is exactly the UX
  > complaint that led to the per-project correction. Under §23 each
  > project gets its own `{project}-coasts/ssg` group in Docker
  > Desktop, so the per-project view is restored.
- **Volume migration friction.** The biggest footgun for adopters.
  Mitigate with a docs recipe (§10.7), `coast ssg doctor` warnings,
  and a future `coast ssg import-host-volume`.
- **State drift.** The SSG's `ssg_services` table and the live DinD
  could disagree (e.g. after manual `docker stop`). `coast ssg ps`
  must inspect live Docker state and reconcile, like
  [`coast-daemon/src/handlers/shared.rs::fetch_shared_services`](../coast-daemon/src/handlers/shared.rs).

## 19. Success criteria

> **Superseded by §23.** The first bullet below encodes the cross-
> project sharing model that is explicitly removed under the
> per-project correction. Current success criterion: two *different*
> projects, each with their own `Coastfile.shared_service_groups`,
> both declaring postgres:5432, run concurrently with zero host-port
> conflict and with their own isolated postgres containers. See §23.

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

`coast-service` stays daemon-agnostic: it does not know or care
whether a given shared service is inline on the local host or served
by the SSG. It receives a `RunRequest::shared_service_ports:
Vec<SharedServicePortForward>` in which every entry names the service
and its **canonical** (inner) port plus a **remote-side dynamic port**
the daemon has allocated on the remote VM:

```rust
pub struct SharedServicePortForward {
    pub name: String,
    pub port: u16,          // canonical port the app dials, e.g. 5432
    pub remote_port: u16,   // dynamic port sshd binds on the remote VM
}
```

On the remote side, [`coast-service/src/handlers/run.rs`](../coast-service/src/handlers/run.rs)
mirrors the local Phase 4 consumer path (§11), not the old Phase 4.5
gateway-IP design. For each forward:

1. Strip the named service from the inner compose project and its
   `depends_on` references.
2. Allocate a docker0 alias IP from the top of the remote DinD's own
   docker0 subnet (same algorithm as
   [`coast-daemon/src/handlers/shared_service_routing.rs::allocate_alias_ip`](../coast-daemon/src/handlers/shared_service_routing.rs)
   — lifted into a shared crate in Phase 18 so both sides call the
   same primitive).
3. Inside the remote DinD, `ip addr add {alias_ip}/{prefix} dev docker0`
   and spawn
   `socat TCP-LISTEN:{port},bind={alias_ip},fork,reuseaddr TCP:host.docker.internal:{remote_port}`.
4. Rewrite every remaining inner-compose service's
   `extra_hosts` to include `{name}:{alias_ip}` so app code that dials
   `postgres` lands on the alias IP inside the DinD's netns.
5. Add `host.docker.internal:host-gateway` on the remote DinD itself
   so the inside-DinD socat can reach `remote_port` on the VM host.

Coast-service has no knowledge of what's listening on the local end of
the reverse tunnel. The daemon owns that: "inline shared service"
vs "SSG dynamic host port" is a local-only distinction (see §20.2).

### 20.2 Local-side allocation

In [`coast-daemon/src/handlers/run/mod.rs::setup_shared_service_tunnels`](../coast-daemon/src/handlers/run/mod.rs),
**both** ends of every reverse-tunnel pair are dynamic:

```rust
// For each fwd in the consumer's shared_service_ports:
//   let remote_port = port_manager::allocate_dynamic_port(…);
//   let local_port  = rewrite_reverse_tunnel_pairs(forwards, &ssg_state)
//                       .get(fwd)  // canonical for inline, SSG dynamic for from_group
//   (remote_port, local_port)
```

The existing
[`coast-daemon/src/handlers/remote/tunnel.rs::reverse_forward_ports`](../coast-daemon/src/handlers/remote/tunnel.rs)
already accepts arbitrary `(remote_port, local_port)` pairs and
spawns:

```text
ssh -R 0.0.0.0:{remote_port}:localhost:{local_port} ...
```

Because every forward gets its own ephemeral `remote_port`, two
coasts on a shared remote cannot collide on a canonical port.
Coast A's postgres tunnel binds remote `61034`; coast B's postgres
tunnel binds remote `59210`; both apps still dial `postgres:5432`
inside their DinDs. See §17 SETTLED #43 for the pre-Phase-18 failure
this replaces.

### 20.3 Flow diagram

```text
REMOTE VM                                    LOCAL HOST
+-------------------------------------+      +-------------------------------+
| Remote DinD                         |      |                               |
|   inner app container               |      |                               |
|     -> postgres:5432                |      |                               |
|     (extra_hosts: postgres:ALIAS)   |      |                               |
|         |                           |      |                               |
|         v                           |      |                               |
|   ALIAS_IP:5432 on docker0          |      |                               |
|   (ip addr add + socat inside DinD) |      |                               |
|         |                           |      |                               |
|         v  socat forwards to        |      |                               |
|         host.docker.internal        |      |                               |
|         :{remote_port}              |      |                               |
|         |                           |      |                               |
|         v                           |      |                               |
| sshd bound 0.0.0.0:{remote_port} ---|--R---|--> ssh client (coast-daemon)  |
+-------------------------------------+      |       |                       |
                                             |       v                       |
                                             | localhost:{local_port}        |
                                             |   inline: canonical port      |
                                             |   SSG:    SSG dynamic port    |
                                             |       |                       |
                                             |       v                       |
                                             |   inline container            |
                                             |   OR SSG DinD publish ->      |
                                             |      inner postgres :5432     |
                                             +-------------------------------+
```

```mermaid
flowchart LR
  subgraph remoteDind [Remote DinD]
    web[inner web] -->|"postgres:5432"| aliasIp["alias IP :5432 on docker0"]
    aliasIp --> socatDind["socat inside DinD"]
  end
  subgraph remoteHost [Remote VM host]
    socatDind -->|"host.docker.internal:remote_port"| sshdListener["sshd -R 0.0.0.0:remote_port"]
  end
  subgraph localHost [Local host]
    sshdListener -.->|"SSH session"| sshClient["ssh client (coast-daemon)"]
    sshClient -->|"localhost:local_port"| localTarget["inline container OR SSG dynamic port"]
  end
```

Canonical<->dynamic translation happens **inside the remote DinD**
(via socat on a docker0 alias IP), identical in shape to the local
Phase 4 consumer topology described in §11. The symmetry is the
point: one mental model works for both local and remote.

### 20.4 `auto_create_db` is always local

Per-instance DB creation for remote consumer coasts happens on the
local machine, against the SSG's inner postgres, via nested
`docker exec` into the coast-ssg container. No exec ever runs on
`coast-service` for SSG services — the remote is purely a consumer of
the reverse tunnel.

### 20.5 Ordering in `handle_remote_run`

[`coast-daemon/src/handlers/run/mod.rs::handle_remote_run`](../coast-daemon/src/handlers/run/mod.rs)
gains one extra step at the front of its pipeline (before the
existing "Starting shared services" phase):

0. Resolve SSG references from the local artifact's Coastfile.
1. If any, ensure SSG is running (auto-start per §11.1, guarded by
   `ssg_mutex`).
2. `setup_shared_service_tunnels` allocates a `remote_port` per
   forward, rewrites the local side of each pair for SSG-backed
   services via `rewrite_reverse_tunnel_pairs`, and spawns
   `ssh -R 0.0.0.0:{remote_port}:localhost:{local_port}` processes
   (§20.2). The allocated `remote_port`s ride on the `RunRequest` so
   coast-service can bind them to its inside-DinD socats (§20.1).
3. Run `auto_create_db` locally against the SSG (§20.4).
4. Forward `RunRequest` to `coast-service`.

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

> **Superseded by §23.** The "singleton DinD runtime" row below is
> the original model; the current contract is one SSG per project,
> named `{project}-ssg`. See §23.

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
| Remote dynamic tunnel port | The per-forward ephemeral port on the remote VM that sshd binds for the reverse tunnel under Phase 18. Independent per coast, so two coasts on one remote can both have a `postgres:5432` consumer without collision (§20). |

---

## Addendum — Missing Plan (Phases 10-17)

Phase 9's post-hoc audit found 13 divergences from §5-§14 and closed
6 in code + 7 via SETTLED entries. A follow-up review found that
several Phase 9 SETTLED entries were LLM-introduced to justify
deferring work that the original DESIGN.md never intended to defer.
The agent shortcutted:

- Scaffolded `coast-ssg/src/port_checkout.rs` in Phase 0 then filled
  the Phase 6 work in `runtime/port_checkout.rs` + `handlers/ssg/checkout.rs`,
  leaving the original stub as dead code with a lingering TODO.
- Left `TODO(ssg-phase-N)` prefixes on `pub mod` comments in
  `coast-ssg/src/lib.rs` after each phase completed.
- Declared the Pattern A lifecycle migration (Phase 9 work) "too
  risky" and stopped after the `SsgDockerOps` trait + 2 pure helpers.
  The full migration was always supposed to happen.
- Marked `file:/path` inject as a "deferred follow-up" in Phase 5
  via §17 #20 / #21, even though DESIGN §14 enumerates `env:` and
  `file:` as equal-class inject targets.
- Left §17 #3, #5, #8 as open questions whose actual Phase
  resolutions never got promoted to SETTLED.

This addendum enumerates the bypassed work as Phases 10-17, each
with the same §0-style checklist. It also picks up the genuinely-
future-but-scoped-for-this-round items DESIGN originally flagged
(`coast ssg import-host-volume` §10.7, `coast ssg checkout-build`
§17-9, SSG Coastfile inheritance §17-7).

### Explicitly out of scope for this addendum

> **First bullet below is explicitly overturned by §23.** Multiple
> concurrent SSGs on one host (one per project) IS the current
> design. The original rationale is preserved for historical context
> but does not describe the current contract.

- **Multiple concurrent SSGs on one host.** The singleton-per-host
  invariant is intentional (§3 non-goal, §2 terminology). Multiple
  *projects* already share the one SSG — that is the core value
  proposition and works today via `from_group = true`. Running two
  separate `coast-ssg` containers side by side is not a feature the
  design aims at; if you need e.g. `postgres:15` and `postgres:16`
  both available, declare them as distinct services within the
  single SSG Coastfile.
- **Remote-resident SSG.** Genuine future work (§3 non-goal,
  §17-6 SETTLED). Larger than a single addendum can absorb and
  orthogonal to the gap-closing items here.

### Phase ordering

The §0 checklists above are the normative source. Suggested
execution order (least-coupled first):

1. Phase 10 — cleanup, 2h
2. Phase 11 — consumer socat refresh (real bug), half day
3. Phase 14 — Phase 9 integration test backfill, half day
4. Phase 13 — `file:/path` inject, 1 day
5. Phase 12 — Pattern A full migration, 2-3 days
6. Phase 15 — import-host-volume, 1 day
7. Phase 16 — checkout-build consumer pinning, 2-3 days
8. Phase 17 — extends / includes, 2-3 days
9. Phase 18 — symmetric remote shared-service routing, 2-3 days
   (land after Phase 17 so the shared-routing crate lift is the
   only large refactor in flight)

---

## 23. Post-v1 correction: per-project SSG

This section documents a fundamental design miss caught during the
`coastguard-platform` migration. §§1-22 describe a singleton SSG
(one container per host, shared across all projects). The intent was
always a per-project SSG (one container per project, isolated state
and volumes). The confusion survived all the way through Phase 18.

§§1-22 and the Addendum are preserved verbatim so the history of the
decision is legible. Every claim in those sections that contradicts
the per-project model now carries a `Superseded by §23` banner.
**This section is the current truth.**

### 23.1 What was built vs what was intended

| Aspect | What §§1-22 built (singleton) | What was intended (per-project) |
|---|---|---|
| Container on the host | One `coast-ssg` DinD, system-wide | One `{project}-ssg` DinD per project (e.g. `cg-ssg`, `filemap-ssg`) |
| Docker Desktop group | `coast-coasts/ssg` (single global group) | `{project}-coasts/ssg` (same group as the project's own coasts) |
| State schema | `ssg(id INTEGER PRIMARY KEY CHECK (id = 1))` | `ssg(project TEXT PRIMARY KEY)` |
| Consumer routing | `from_group = true` resolves against the single global SSG | `from_group = true` resolves against the consumer's own project's SSG |
| Cross-project sharing | Core feature: two projects reference the same postgres | Explicitly removed: each project has its own postgres |
| Data isolation | Single DB instance shared across projects (projects create distinct databases inside it) | Each project's postgres is isolated; same canonical port, different containers |
| `Coastfile.shared_service_groups` | One per host (whichever `ssg build` ran last wins) | One per project, keyed by the sibling `Coastfile`'s `[coast] name` |
| `coast ssg <verb>` | Global operations | Operate on the project resolved from cwd (like `coast build`/`coast run`) |
| Remote SSGs | Non-goal; local singleton only | Per-`(project, remote)` pair; each project's remote coasts route to that project's SSG |

### 23.2 Why the singleton model is wrong

Three concrete failures, in order of visibility:

1. **Docker Desktop UX regression.** The singleton shows up as
   `coast-coasts/ssg`, divorced from the project's own compose group
   (`cg-coasts`, `filemap-coasts`, etc.). Users looking at Docker
   Desktop cannot tell which project's databases they're staring at.
   §18 Risks called this out as an "accepted tradeoff." It should not
   have been accepted.
2. **Volume ownership is project-local, but the SSG is not.** The
   inline shared-services model uses named volumes like
   `cg_postgres_data`. Those names encode the project. The SSG takes
   those same volumes but moves them into a global container — the
   project prefix becomes meaningless, and the volume's
   `com.docker.compose.project` label points at `coast-ssg`, not at
   the project that owns the data.
3. **No isolation for divergent project needs.** If project A wants
   postgres:15 and project B wants postgres:16, the singleton forces
   them to either coexist as distinct service names (`postgres15`,
   `postgres16`) or to coordinate image selection globally. Neither
   is the right answer for independent projects.

The root cause of the miss was conflating two different problems:

- *Host-port collisions* — two projects both trying to bind the
  host's port 5432. Solvable with dynamic host ports alone.
- *Cross-project database sharing* — two projects wanting to talk to
  the same postgres data. A feature, but one nobody asked for.

The singleton design solved both at once, assuming the second
followed automatically from the first. It doesn't. Dynamic host ports
plus per-project SSGs solve the collision problem without forcing
data sharing. Data sharing is a distinct (future) feature that, if
needed at all, should be an opt-in between two independently running
SSGs — not a baked-in global default.

### 23.3 Corrected contract

1. **One SSG per project**, keyed by the consumer project's
   `[coast] name` (from the sibling `Coastfile`).
2. **Container name**: `{project}-ssg` (e.g. `cg-ssg`).
3. **Compose-project label** on the outer SSG DinD: `{project}-coasts`,
   producing the Docker Desktop group `{project}-coasts/ssg`. This
   drops the SSG into the same visual group as the project's coasts.
4. **`Coastfile.shared_service_groups`** is per-project. Located
   next to the consumer `Coastfile`. Project inferred from the sibling
   Coastfile's `[coast] name`, or stated explicitly via
   `[ssg] project = "..."` (must match sibling if both present).
5. **Consumer routing** for `[shared_services.<name>] from_group = true`:
   looks up ONLY the consumer's own project's SSG. Missing SSG or
   missing service name = hard error with a project-scoped message.
   Never falls through to another project's SSG.
6. **`coast ssg <verb>`** resolves the current project from cwd using
   the same helper as `coast build`/`coast run`. Every daemon request
   carries a `project: String`. A new `coast ssg ls` lists every SSG
   across projects.
7. **Remote SSGs** follow the same per-project model: one SSG per
   `(project, remote)` pair. The Phase 18 symmetric reverse-tunnel
   plumbing already keys its state by `(project, instance)`, so this
   is mainly threading the project key through the port resolution
   step.
8. **Port allocation** stays dynamic. Each SSG gets its own
   `allocate_dynamic_port` call; collisions between two running SSGs
   are impossible because each picks a distinct free port.
9. **Volumes** stay as declared in the SSG Coastfile. They are now
   owned by `{project}-ssg`'s compose project label, matching the
   project's other coasts in Docker Desktop.
10. **Cross-project sharing is not supported.** Two projects that
    need shared data must coordinate at the application layer (e.g.
    explicit network setup between their SSGs) or, in a future
    revision, via an explicit opt-in syntax. §1 Motivation #3 ("no
    cross-project sharing") stops being a problem worth solving.

### 23.4 Migration posture

Pre-launch. No users. No on-disk data migration. When the Phase 20
schema change lands, the daemon wipes the old singleton rows from
`ssg`, `ssg_services`, and `ssg_port_checkouts` on startup. Running
coasts that reference the old SSG must be stopped and rerun under the
new code. Phase 18.5 of the correction runbook handles the one-shot
cleanup.

### 23.5 Sections superseded by §23 (quick reference)

| Section | What it still gets right | What §23 changes |
|---|---|---|
| §1 Motivation | Pain #1 (host-port collisions), #2 (Docker Desktop sprawl), #4 (`auto_create_db` project-bound) are still real pain points; solved by §23. | Pain #3 ("no cross-project sharing") is no longer a problem worth solving. |
| §2 Terminology | Definitions of SSG, SSG service, SSG Coastfile, canonical port, SSG host port, consumer coast. | "singleton" → "per-project"; one SSG per host → one per project. |
| §3 Goals / Non-goals | Goals 2-6 + non-goals 2-4. | Goal "One SSG per host" flips to "One SSG per project". Non-goal "Multiple concurrent SSGs on one host" is dropped (it's now the design). |
| §4 High-level architecture | The alias-IP + socat + dynamic-port routing chain. | Diagram: `coast-ssg` → `{project}-ssg`; two projects get two SSGs, not one shared one. |
| §8 Daemon state | Table columns for `container_id`, `build_id`, dynamic ports; the `ssg_consumer_pins` table is already project-keyed and stays. | `ssg.id INTEGER PRIMARY KEY CHECK (id = 1)` → `ssg.project TEXT PRIMARY KEY`; `ssg_services` and `ssg_port_checkouts` grow `project` column in the composite PK. |
| §11 Port plumbing | The full alias-IP + socat + `host.docker.internal` chain is unchanged. | Step 2 lookup is scoped to the consumer's own project's SSG. |
| §18 Risks | Hidden port collisions, volume migration friction, state drift all still apply. | "Docker Desktop UX regression" bullet is obsolete — the correction restores the per-project view. |
| §19 Success criteria | Criteria 2-5 stand. | Criterion 1 (three projects sharing one SSG) is replaced with "two projects, each with own SSG, same canonical port, no host-port conflict, data isolation maintained." |
| §20 Remote coasts with SSG | Phase 18 symmetric routing, `(project, instance)`-keyed state, `auto_create_db` locality. | Phase 24 threads project through the SSG port resolution so each remote coast routes to its own project's SSG. |
| §22 Terminology cheat sheet | Every row except "SSG" is still correct. | "SSG — Shared Service Group — the singleton DinD runtime" → "...the per-project DinD runtime, one per consumer project". |
| Addendum "Out of scope" | "Remote-resident SSG" bullet stands (still future work). | "Multiple concurrent SSGs on one host" bullet is explicitly overturned — that IS the design under §23. |
