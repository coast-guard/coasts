# Building a Shared Service Group

`coast ssg build` parses a `Coastfile.shared_service_groups` file, pulls every declared image into the shared host image cache, and writes a versioned build artifact under `~/.coast/ssg/builds/{build_id}/`. The command is non-destructive toward the running SSG -- the next `coast ssg run` or `coast ssg start` picks up the new build, but an already-running SSG keeps serving its current build until you restart it.

For the full TOML schema see [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md).

## Discovery

`coast ssg build` finds its Coastfile using the same rules as `coast build`:

- With no flags, it looks in the current working directory for `Coastfile.shared_service_groups` or `Coastfile.shared_service_groups.toml`. Both forms are equivalent and the `.toml` suffix wins when both exist.
- `-f <path>` / `--file <path>` points at an arbitrary file.
- `--working-dir <dir>` decouples the project root from the Coastfile location (same flag as `coast build --working-dir`).
- `--config '<inline-toml>'` supports scripting and CI flows where you synthesize the Coastfile in-line.

```bash
coast ssg build
coast ssg build -f /shared/coast/Coastfile.shared_service_groups
coast ssg build --working-dir /shared/coast
coast ssg build --config '[shared_services.pg]
image = "postgres:16"
ports = [5432]'
```

## What Build Does

Each `coast ssg build` streams progress through the same `BuildProgressEvent` channel as `coast build`, so the CLI renders `[N/M]` step counters.

1. **Parse** the `Coastfile.shared_service_groups`. Only `[ssg]` and `[shared_services.*]` sections are accepted. Any other top-level key is rejected. Volume entries are split into host bind mounts and inner named volumes (see [Volumes](VOLUMES.md)).
2. **Resolve the build id.** The id has the shape `{coastfile_hash}_{YYYYMMDDHHMMSS}`. The hash incorporates the raw source plus a deterministic summary of the parsed services, so any change produces a new id.
3. **Synthesize the inner `compose.yml`.** Every `[shared_services.*]` block becomes an entry in a single Docker Compose file. This is the file the SSG's inner Docker daemon runs via `docker compose up -d` at `coast ssg run` time.
4. **Pull and cache each image.** Images are stored as OCI tarballs in `~/.coast/image-cache/`, the same pool `coast build` uses. Cache hits from either command speed up the other.
5. **Write the build artifact** to `~/.coast/ssg/builds/{build_id}/` with three files: `manifest.json`, `ssg-coastfile.toml`, and `compose.yml` (see layout below).
6. **Flip the `latest` symlink.** `~/.coast/ssg/latest` atomically points at the new build directory.
7. **Auto-prune** older builds to the 5 most recent. Earlier artifact directories are removed from disk.

## Artifact Layout

```text
~/.coast/
  ssg/
    latest -> builds/b455787d95cfdeb_20260420061903    (symlink)
    builds/
      b455787d95cfdeb_20260420061903/                   (the new build)
        manifest.json
        ssg-coastfile.toml
        compose.yml
      a1c7d783e4f56c9a_20260419184221/                  (prior build)
        ...
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
      "env_keys": ["POSTGRES_PASSWORD", "POSTGRES_USER"],
      "volumes": ["/var/coast-data/postgres:/var/lib/postgresql/data"],
      "auto_create_db": true
    }
  ]
}
```

Env values are intentionally absent -- only env variable names are captured, matching the safety posture of `coast build` manifests. Secrets live in the daemon's keystore, not in artifact files.

`ssg-coastfile.toml` is the parsed, interpolated, post-validation Coastfile. It is byte-identical to what the daemon would have seen at parse time. Useful for auditing a past build.

`compose.yml` is what the SSG's inner Docker daemon runs. See the [Volumes page](VOLUMES.md) for the synthesis rules, especially the symmetric-path bind mount strategy.

## Inspecting a Build Without Running It

`coast ssg ps` reads `manifest.json` directly -- it does not inspect any container. You can run it immediately after `coast ssg build` to see the services that will start on the next `coast ssg run`:

```bash
coast ssg ps

# SSG build: b455787d95cfdeb_20260420061903
#
#   SERVICE              IMAGE                          PORT       STATUS
#   postgres             postgres:16                    5432       built
#   redis                redis:7-alpine                 6379       built
```

The `PORT` column is the inner container port. Dynamic host ports are allocated later, at `coast ssg run`. See [Lifecycle](LIFECYCLE.md) for the run-time picture.

## Rebuilds and Drift

A consumer Coast's `coast build` records the active SSG's `build_id` plus the image refs for every `from_group = true` service it references. At `coast run` time the daemon checks that snapshot against the current SSG. A full explanation lives in [Consuming -> Drift Detection](CONSUMING.md#drift-detection); in short:

- Identical `build_id` -> proceed silently.
- Different `build_id` but identical image refs for every referenced service -> warn and proceed.
- Image ref changed or a referenced service is missing -> hard error; rebuild the Coast.

Rebuilds to the SSG therefore do not automatically cascade into consumer Coasts. Consumers opt in on their own schedule by running `coast build` again.
