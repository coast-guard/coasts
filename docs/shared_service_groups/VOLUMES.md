# SSG Volumes

Inside `[shared_services.<name>]`, the `volumes` array uses the standard Docker Compose syntax:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

A leading `/` means a **host bind path** -- bytes live on the host filesystem and the inner service reads and writes them in place. Without a leading slash, e.g. `pg_wal:/var/lib/postgresql/wal`, the source is a **Docker named volume that lives inside the SSG's nested Docker daemon** -- it survives `coast ssg rm` and is dropped by `coast ssg rm --with-data`. Both forms are accepted.

Rejected at parse: relative paths (`./data:/...`), `..` components, container-only volumes (no source), and duplicate targets within one service.

## Reusing a Docker volume from docker-compose or an inline shared service

If you already have data sitting inside a host Docker named volume -- from `docker-compose up`, from an inline `[shared_services.postgres] volumes = ["infra_postgres_data:/..."]`, or from a hand-rolled `docker volume create` -- you can have the SSG read the same bytes by bind-mounting the volume's underlying host directory:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

The left side is the host filesystem path of an existing Docker volume; `docker volume inspect <name>` reports it as the `Mountpoint` field. Coast doesn't copy bytes -- the SSG reads and writes the same files docker-compose did. `coast ssg rm` (without `--with-data`) leaves the volume untouched, so docker-compose can keep using it too.

> **Why not just `infra_postgres_data:/var/lib/postgresql/data`?** That works for inline `[shared_services.*]` (the volume gets created on the host Docker daemon, where docker-compose can see it). It does *not* work the same way inside an SSG -- a name without a leading slash creates a fresh volume inside the SSG's nested Docker daemon, isolated from the host. Use the volume's mountpoint path instead when you want to share data with anything that runs on the host daemon.

### `coast ssg import-host-volume`

`coast ssg import-host-volume` resolves the volume's `Mountpoint` via `docker volume inspect` and emits (or applies) the equivalent `volumes` line, so you don't hand-construct the `/var/lib/docker/volumes/<name>/_data` path.

Snippet mode (default) prints the TOML fragment to paste:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data
```

The output is a `[shared_services.postgres]` block with the new `volumes = [...]` entry already merged in:

```text
# Add the following to Coastfile.shared_service_groups (infra_postgres_data -> /var/lib/postgresql/data):

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_PASSWORD = "coast" }

# Bind line: /var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data
```

Apply mode rewrites `Coastfile.shared_service_groups` in place and saves the original to `Coastfile.shared_service_groups.bak`:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data \
    --apply
```

Flags:

- `<VOLUME>` (positional) -- host Docker named volume. Must already exist (`docker volume inspect` is the check); create or rename with `docker volume create` first otherwise.
- `--service` -- the `[shared_services.<name>]` section to edit. The section must already exist.
- `--mount` -- absolute container path. Relative paths are rejected. Duplicate mount paths on the same service are hard-errors.
- `--file` / `--working-dir` / `--config` -- SSG Coastfile discovery, same rules as `coast ssg build`.
- `--apply` -- rewrite the Coastfile in place. Cannot be combined with `--config` (inline text has nothing to write back to).

The `.bak` file contains the original bytes verbatim, so you can recover the exact pre-apply state.

`/var/lib/docker/volumes/<name>/_data` is the path Docker has used as a volume mountpoint for many years and is what `docker volume inspect` reports today. Docker doesn't formally promise to keep this path forever; if a future Docker release moves volumes elsewhere, re-run `coast ssg import-host-volume` to pick up the new path.

## Permissions

Several images refuse to start when their data directory is owned by the wrong user. Postgres (UID 999 in the debian tag, UID 70 in the alpine tag), MySQL/MariaDB (UID 999), and MongoDB (UID 999) are the common offenders. If the host directory is owned by root, Postgres exits at startup with a terse "data directory has wrong ownership".

The fix is one command:

```bash
# postgres:16 (debian)
sudo chown -R 999:999 /var/coast-data/postgres

# postgres:16-alpine
sudo chown -R 70:70 /var/coast-data/postgres
```

Run this before `coast ssg run`. If the directory does not exist yet, `coast ssg run` creates it with default ownership (root on Linux, your user on macOS through Docker Desktop). That default is usually wrong for Postgres. If you came in via `coast ssg import-host-volume` and `docker-compose up` had previously chown'd the volume on first start, you're already fine.

## `coast ssg doctor`

`coast ssg doctor` is a read-only check that runs against the current project's SSG (resolved from the cwd `Coastfile`'s `[coast].name` or `--working-dir`). It prints one finding per `(service, host-bind)` pair in the active build, plus secret-extraction findings (see [Secrets](SECRETS.md)).

For each known image (Postgres, MySQL, MariaDB, MongoDB) it consults a built-in UID/GID table, compares against `stat(2)` on each host path, and emits:

- `ok` when the owner matches the image's expectation.
- `warn` when it diverges. The message includes the `chown` command to fix it.
- `info` when the directory does not exist yet, or when the matching image has only named volumes (nothing to check from the host side).

Services whose images aren't in the known-image table are silently skipped. Forks like `ghcr.io/baosystems/postgis` are not flagged -- the doctor would rather say nothing than emit a wrong warning.

```bash
coast ssg doctor
```

Sample output with a mismatched Postgres directory:

```text
SSG 'b455787d95cfdeb_20260420061903' (project cg): 1 warning(s), 0 ok, 0 info. Fix the warnings before `coast ssg run`.

  LEVEL   SERVICE              PATH                                     MESSAGE
  warn    postgres             /var/coast-data/postgres                 Owner 0:0 but postgres expects 999:999. Run `sudo chown -R 999:999 /var/coast-data/postgres` before `coast ssg run`.
```

Doctor doesn't modify anything. Permissions on bytes you put on your host filesystem aren't something Coast silently mutates.

## Platform notes

- **macOS Docker Desktop.** Raw host paths must be listed under Settings -> Resources -> File Sharing. Defaults include `/Users`, `/Volumes`, `/private`, `/tmp`. `/var/coast-data` is **not** in the default list on macOS -- prefer `$HOME/coast-data/...` for fresh paths, or add `/var/coast-data` to File Sharing. The `/var/lib/docker/volumes/<name>/_data` form is *not* a host path -- Docker resolves it inside its own VM -- so it works without a File Sharing entry.
- **WSL2.** Prefer WSL-native paths (`~`, `/mnt/wsl/...`). `/mnt/c/...` works but is slow because of the 9P protocol bridging the Windows host filesystem.
- **Linux.** No gotchas.

## Lifecycle

- `coast ssg rm` -- removes the SSG's outer DinD container. **Volume contents are untouched**, host bind-mount contents are untouched, keystore is untouched. Anything else that uses the same Docker volume keeps working.
- `coast ssg rm --with-data` -- drops volumes that live **inside the SSG's nested Docker daemon** (the `name:path` form without a leading slash). Host bind mounts and external Docker volumes are still untouched -- Coast doesn't own them.
- `coast ssg build` -- never touches volumes. Only writes a manifest and (when `[secrets]` is declared) keystore rows.
- `coast ssg run` / `start` / `restart` -- creates host bind-mount directories if they don't exist (with default ownership -- see [Permissions](#permissions)).

## See Also

- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) -- full TOML schema including volume syntax
- [Volume Topology](../concepts_and_terminology/VOLUMES.md) -- shared, isolated, and snapshot-seeded volume strategies for non-SSG services
- [Building](BUILDING.md) -- where the manifest comes from
- [Lifecycle](LIFECYCLE.md) -- when volumes are created, stopped, and removed
- [Secrets](SECRETS.md) -- file-injected secrets land at `~/.coast/ssg/runs/<project>/secrets/<basename>` and bind-mount into inner services read-only
