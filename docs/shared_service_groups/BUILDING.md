# Building a Shared Service Group

`coast ssg build` parses your project's `Coastfile.shared_service_groups`, extracts any declared secrets, pulls every image into the host image cache, and writes a versioned build artifact under `~/.coast/ssg/<project>/builds/<build_id>/`. The command is non-destructive toward an already-running SSG -- the next `coast ssg run` or `coast ssg start` picks up the new build, but a running `<project>-ssg` keeps serving its current build until you restart it.

The project name comes from `[coast].name` in the sibling `Coastfile`. Each project has its own SSG named `<project>-ssg`, its own build directory, and its own `latest_build_id` -- there is no host-wide "current SSG."

For the full TOML schema see [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md).

## Discovery

`coast ssg build` finds its Coastfile using the same rules as `coast build`:

- With no flags, it looks in the current working directory for `Coastfile.shared_service_groups` or `Coastfile.shared_service_groups.toml`. Both forms are equivalent and the `.toml` suffix wins when both exist.
- `-f <path>` / `--file <path>` points at an arbitrary file.
- `--working-dir <dir>` decouples the project root from the Coastfile location (same flag as `coast build --working-dir`).
- `--config '<inline-toml>'` supports scripting and CI flows where you synthesize the Coastfile in-line.

```bash
coast ssg build
coast ssg build -f /path/to/Coastfile.shared_service_groups
coast ssg build --working-dir /shared/coast
coast ssg build --config '[shared_services.pg]
image = "postgres:16"
ports = [5432]'
```

The build resolves the project name from the sibling `Coastfile` in the same directory. If you use `--config` (no on-disk Coastfile.shared_service_groups), the cwd must still contain a `Coastfile` whose `[coast].name` is the SSG project.

## What Build Does

Each `coast ssg build` streams progress through the same `BuildProgressEvent` channel as `coast build`, so the CLI renders `[N/M]` step counters.

1. **Parse** the `Coastfile.shared_service_groups`. `[ssg]`, `[shared_services.*]`, `[secrets.*]`, and `[unset]` are the accepted top-level sections. Volume entries are split into host bind mounts and inner named volumes (see [Volumes](VOLUMES.md)).
2. **Resolve the build id.** The id has the shape `{coastfile_hash}_{YYYYMMDDHHMMSS}`. The hash folds in the raw source, a deterministic summary of the parsed services, and the `[secrets.*]` config (so editing a secret's `extractor` or `var` produces a new id).
3. **Synthesize the inner `compose.yml`.** Every `[shared_services.*]` block becomes an entry in a single Docker Compose file. This is the file the SSG's inner Docker daemon runs via `docker compose up -d` at `coast ssg run` time.
4. **Extract secrets.** When `[secrets.*]` is non-empty, run each declared extractor and store the encrypted result in `~/.coast/keystore.db` under `coast_image = "ssg:<project>"`. Skipped silently when the Coastfile has no `[secrets]` block. See [Secrets](SECRETS.md) for the full pipeline.
5. **Pull and cache each image.** Images are stored as OCI tarballs in `~/.coast/image-cache/`, the same pool `coast build` uses. Cache hits from either command speed up the other.
6. **Write the build artifact** to `~/.coast/ssg/<project>/builds/<build_id>/` with three files: `manifest.json`, `ssg-coastfile.toml`, and `compose.yml` (see layout below).
7. **Update the project's `latest_build_id`.** This is a state-database flag, not a filesystem symlink. `coast ssg run` and `coast ssg ps` read it to know which build to operate on.
8. **Auto-prune** older builds to the 5 most recent for this project. Earlier artifact directories under `~/.coast/ssg/<project>/builds/` are removed from disk. Pinned builds (see "Locking a project to a specific build" below) are always preserved.

## Artifact Layout

```text
~/.coast/
  keystore.db                                          (shared, namespaced by coast_image)
  keystore.key
  image-cache/                                         (shared OCI tarball pool)
  ssg/
    cg/                                                (project "cg")
      builds/
        b455787d95cfdeb_20260420061903/                (the new build)
          manifest.json
          ssg-coastfile.toml
          compose.yml
        a1c7d783e4f56c9a_20260419184221/               (prior build)
          ...
    filemap/                                           (project "filemap" -- separate tree)
      builds/
        ...
    runs/
      cg/                                              (per-project run scratch)
        compose.override.yml                           (rendered at coast ssg run)
        secrets/<basename>                             (file-injected secrets, mode 0600)
```

`manifest.json` captures the build metadata that downstream code cares about:

```json
{
  "build_id": "b455787d95cfdeb_20260420061903",
  "built_at": "2026-04-20T06:19:03Z",
  "coastfile_hash": "b455787d95cfdeb",
  "services": [
    {
      "name": "postgres",
      "image": "postgres:16",
      "ports": [5432],
      "env_keys": ["POSTGRES_USER", "POSTGRES_DB"],
      "volumes": ["pg_data:/var/lib/postgresql/data"],
      "auto_create_db": true
    }
  ],
  "secret_injects": [
    {
      "secret_name": "pg_password",
      "inject_type": "env",
      "inject_target": "POSTGRES_PASSWORD",
      "services": ["postgres"]
    }
  ]
}
```

Env values and secret payloads are intentionally absent -- only env variable names and inject *targets* are captured. Secret values live encrypted in the keystore, never in artifact files.

`ssg-coastfile.toml` is the parsed, interpolated, post-validation Coastfile. It is byte-identical to what the daemon would have seen at parse time. Useful for auditing a past build.

`compose.yml` is what the SSG's inner Docker daemon runs. See [Volumes](VOLUMES.md) for the synthesis rules, especially the symmetric-path bind mount strategy.

## Inspecting a Build Without Running It

`coast ssg ps` reads `manifest.json` for the project's `latest_build_id` directly -- it does not inspect any container. You can run it immediately after `coast ssg build` to see the services that will start on the next `coast ssg run`:

```bash
coast ssg ps

# SSG build: b455787d95cfdeb_20260420061903 (project: cg)
#
#   SERVICE              IMAGE                          PORT       STATUS
#   postgres             postgres:16                    5432       built
#   redis                redis:7-alpine                 6379       built
```

The `PORT` column is the inner container port. Dynamic host ports are allocated at `coast ssg run`; the consumer-facing virtual port is reported by `coast ssg ports`. See [Routing](ROUTING.md) for the full picture.

To browse every build for a project (with timestamps, service counts, and which build is currently latest), use:

```bash
coast ssg builds-ls
```

## Rebuilds

A new `coast ssg build` is the canonical way to update an SSG. It re-extracts secrets (if any), updates `latest_build_id`, and prunes old artifacts. Consumers don't auto-rebuild -- their `from_group = true` references resolve at consumer-build time against whatever build was current then. To roll a consumer onto a newer SSG, run `coast build` for the consumer.

The runtime is forgiving across rebuilds: virtual ports stay stable per `(project, service, container_port)`, so consumers don't need to be refreshed for routing. Shape changes (a service was renamed or removed) surface as connection errors at the consumer level, not as a Coast-level "drift" message. See [Routing](ROUTING.md) for the why.

## Locking a project to a specific build

By default the SSG runs the project's `latest_build_id`. If you need to freeze a project on an earlier build -- for regression repro, A/B comparing two builds across worktrees, or holding a long-lived branch on a known-good shape -- use the pin commands:

```bash
coast ssg checkout-build <build_id>     # pin this project to <build_id>
coast ssg show-pin                      # report the active pin (if any)
coast ssg uncheckout-build              # release the pin; back to latest
```

Pins are per consumer project (one pin per project, shared across worktrees). When pinned:

- `coast ssg run` auto-starts the pinned build instead of `latest_build_id`.
- `coast build` validates `from_group` references against the pinned build's manifest.
- `auto_prune` will not delete the pinned build directory, even if it falls outside the 5-most-recent window.

The Coastguard SPA shows a `PINNED` badge next to the build id when a pin is active, and `LATEST` when not. The pin commands also appear in [CLI](CLI.md).
