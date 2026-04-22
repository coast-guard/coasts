# SSG Volumes

Volumes are the most opinionated part of the SSG design. The SSG runs inside a singleton DinD container, and the services it manages run one level deeper, inside the SSG's own inner Docker daemon. Two layers of bind mounts have to agree for host data to actually reach the inner Postgres / Redis / MongoDB process. This page documents the rules.

## Declaration Shapes

Inside `[shared_services.<name>]`, the `volumes` array accepts two forms:

```toml
[shared_services.postgres]
image = "postgres:16"
volumes = [
    "/var/coast-data/postgres:/var/lib/postgresql/data",   # host bind mount
    "pg_wal:/var/lib/postgresql/wal",                       # inner named volume
]
```

- **Host bind mount.** The source starts with `/`. Bytes live on your real host filesystem. Host agents, `ls`, `du`, backups all see the same bytes the inner service sees.
- **Inner named volume.** The source is a Docker volume name (no `/`). The volume lives inside the SSG's inner Docker daemon. It survives SSG restarts (the inner daemon's `/var/lib/docker` is a named host volume), but is opaque to the host.

Rejected at parse time: relative paths (`./data:/...`), `..` components, duplicate targets within one service, and container-only volumes with no source.

## The Symmetric-Path Plan

When you write `"/var/coast-data/postgres:/var/lib/postgresql/data"`, the daemon uses the same host path string in both bind hops.

**Hop 1 -- outer DinD creation.** The `coast-ssg` container is created with `-v /var/coast-data/postgres:/var/coast-data/postgres`. The host source and the DinD-visible destination are the same string. After this hop, `/var/coast-data/postgres` exists inside the DinD and reads and writes pass through to the host.

**Hop 2 -- inner compose.** The synthesized `compose.yml` declares `- /var/coast-data/postgres:/var/lib/postgresql/data` for the Postgres service. The inner Docker daemon resolves `/var/coast-data/postgres` in its own filesystem view, which is the DinD container's filesystem, which is the host directory thanks to hop 1. Same inode, same bytes, three names for one thing.

```text
+-- Host filesystem ----------------------------------+
| /var/coast-data/postgres/         (real dir)        |
| |-- base/  PG_VERSION  ...                          |
+-----------------------------------------------------+
    | Hop 1: -v /var/coast-data/postgres:/var/coast-data/postgres
    v
+-- coast-ssg DinD container -------------------------+
| /var/coast-data/postgres/         (same inodes)     |
| /var/lib/docker/                  (named volume)    |
| Inner dockerd runs here.                            |
+-----------------------------------------------------+
    | Hop 2: - /var/coast-data/postgres:/var/lib/postgresql/data
    v
+-- Inner postgres container -------------------------+
| /var/lib/postgresql/data/         (same inodes)     |
+-----------------------------------------------------+
```

Why symmetric paths, rather than remapping to `/coast-ssg-vols/{svc}/{i}`:

- Log legibility. Postgres errors that cite `/var/lib/postgresql/data/base/1/...` are traceable by `ls /var/coast-data/postgres/base/1/...` on the host without any mental translation.
- Error messages echo user intent.
- No synth-side naming scheme to maintain.
- `grep` friendly. The user's path appears verbatim in their Coastfile and everywhere else.

## Inner Named Volumes

Named-volume entries (`"pg_wal:/var/lib/postgresql/wal"`) persist inside the SSG's own Docker daemon. Their on-disk representation lives under `/var/lib/docker/volumes/` inside the DinD container, which the SSG's outer named volume (`coast-dind--coast--ssg`) backs to the host. Practical consequences:

- Named volumes survive `coast ssg stop` and `coast ssg start`.
- Named volumes survive `coast ssg rm` by default.
- `coast ssg rm --with-data` drops them before removing the DinD.
- Named volumes are opaque to the host -- you cannot `ls` into them from outside the SSG.

Use named volumes when you want a clean, Docker-managed home for auxiliary state (write-ahead logs, temporary indexes) and do not need host visibility. Use host bind mounts for data you want to inspect, back up, or share with host tools.

## Permissions Caveat

Several images refuse to start when their data directory is owned by the wrong user. Postgres (UID 999 in the debian tag, UID 70 in the alpine tag), MySQL/MariaDB (UID 999), and MongoDB (UID 999) are the common offenders. If the host directory is owned by root, Postgres exits at startup with a terse "data directory has wrong ownership".

The fix is one command:

```bash
# postgres:16 (debian)
sudo chown -R 999:999 /var/coast-data/postgres

# postgres:16-alpine
sudo chown -R 70:70 /var/coast-data/postgres
```

Run this before `coast ssg run`. If the directory does not exist yet, `coast ssg run` creates it with default ownership (root on Linux, your user on macOS through Docker Desktop). That default is usually wrong for Postgres.

## `coast ssg doctor`

`coast ssg doctor` is a read-only check that prints one finding per `(service, host-bind-mount)` pair in the active SSG build. It consults a built-in table of known images (Postgres, MySQL, MariaDB, MongoDB) and their expected UID/GID, compares against `stat(2)` on each host path, and emits:

- `ok` when the owner matches the image's expectation.
- `warn` when it diverges. The message includes the `chown` command to fix it.
- `info` when the directory does not exist yet, or when the matching image has only named volumes (nothing to check from the host side).

Services whose images are not in the known-image table are silently skipped. Forks like `ghcr.io/baosystems/postgis` are not flagged -- the doctor would rather say nothing than emit a wrong warning.

```bash
coast ssg doctor
```

Sample output with a mismatched Postgres directory:

```text
SSG 'b455787d95cfdeb_20260420061903': 1 warning(s), 0 ok, 0 info. Fix the warnings before `coast ssg run`.

  LEVEL   SERVICE              PATH                                     MESSAGE
  warn    postgres             /var/coast-data/postgres                 Owner 0:0 but postgres expects 999:999. Run `sudo chown -R 999:999 /var/coast-data/postgres` before `coast ssg run`.
```

Doctor does not modify anything. Permissions on bytes you put on your host filesystem are not something Coast silently mutates. Run the `chown` command it suggests and re-run doctor to verify.

## Platform Notes

- **macOS Docker Desktop.** Host paths must be listed under Settings -> Resources -> File Sharing. Defaults include `/Users`, `/Volumes`, `/private`, `/tmp`. `/var/coast-data` is **not** in the default list on macOS. Prefer `$HOME/coast-data/...` in your Coastfile on macOS, or add `/var/coast-data` to File Sharing.
- **WSL2.** Prefer WSL-native paths (`~`, `/mnt/wsl/...`). `/mnt/c/...` works but is slow because of the 9P protocol that bridges the Windows host filesystem.
- **Linux.** No gotchas.

## Host-Volume Migration Recipe

If you already have data inside a host Docker named volume (for example `infra_postgres_data:/var/lib/postgresql/data`), you can migrate to the SSG without copying any bytes. Bind-mount the volume's underlying host directory directly:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

Caveats:

- `/var/lib/docker/volumes/<name>/_data` is an internal Docker path. It exists today but is not an API Docker promises to keep stable. Treat the migration as one-time, not as a long-term deployment shape.
- Postgres running as UID 999 still needs the directory's ownership to match. If `docker-compose` previously chown'd the volume, you are already fine. If not, run `sudo chown -R 999:999 /var/lib/docker/volumes/infra_postgres_data/_data` first.
- After the migration, consider copying the data out to a dedicated path (`/var/coast-data/postgres`) once you have confirmed the SSG is serving correctly. That decouples the SSG from Docker's internal volume layout.

### Automating the recipe: `coast ssg import-host-volume`

`coast ssg import-host-volume` resolves the volume's `Mountpoint` via `docker volume inspect` and emits (or applies) the equivalent bind-mount line for you, so you do not have to hand-construct the `/var/lib/docker/volumes/<name>/_data` path.

Snippet mode (default) prints the TOML fragment to paste:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data
```

The output is a `[shared_services.postgres]` block with the new `volumes = [...]` entry already merged in, plus a one-line summary of the resolved bind:

```text
# Add the following to Coastfile.shared_service_groups (infra_postgres_data -> /var/lib/postgresql/data):

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = [
    "pg_data:/var/lib/postgresql/data-existing",
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
- `--service` -- the `[shared_services.<name>]` section to edit. The section must already exist; this command adds to an existing service, not a new one.
- `--mount` -- absolute container path. Relative paths are rejected. Duplicate mount paths on the same service are hard-errors (remove the existing entry first or pick a different container path).
- `--file` / `--working-dir` / `--config` -- SSG Coastfile discovery, same rules as `coast ssg build`.
- `--apply` -- rewrite the Coastfile in place. Cannot be combined with `--config` (inline text has nothing to write back to).

The `.bak` file contains the original bytes verbatim (not the re-serialized output), so you can recover the exact pre-apply state if needed.

See [DESIGN §10.7](https://github.com/coasts-dev/coasts/blob/main/coast-ssg/DESIGN.md) for the full design and [SETTLED #40](https://github.com/coasts-dev/coasts/blob/main/coast-ssg/DESIGN.md) for the design decisions (bollard vs. subprocess, reuse of the parser+serializer round-trip for apply mode, duplicate-rejection policy).

## Lifecycle Summary

- `coast ssg rm` -- inner named volumes survive, host bind mount contents are untouched.
- `coast ssg rm --with-data` -- inner named volumes are dropped, host bind mount contents are untouched.
- `coast ssg build` -- never touches volumes. Only writes a manifest.
- `coast ssg run` / `start` / `restart` -- creates host bind mount directories if they do not exist (with default ownership -- see the permissions caveat).

## See Also

- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) -- full TOML schema including volume syntax
- [Volume Topology](../concepts_and_terminology/VOLUMES.md) -- shared, isolated, and snapshot-seeded volume strategies for non-SSG services
- [Building](BUILDING.md) -- where the manifest comes from
- [Lifecycle](LIFECYCLE.md) -- when volumes are created, stopped, and removed
