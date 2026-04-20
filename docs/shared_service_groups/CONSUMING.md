# Consuming a Shared Service Group

Projects opt into an SSG-owned service per service, per project, using a one-line flag in the consumer's Coastfile. Inside the Coast, app containers still see `postgres:5432`; the socat-based routing layer that already handles inline shared services just points at the SSG's dynamic host port instead.

## Syntax

Add a `[shared_services.<name>]` block with `from_group = true`:

```toml
# Consumer Coastfile
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true

# Optional per-project overrides:
inject = "env:DATABASE_URL"
# auto_create_db = true       # overrides the SSG service's default
```

The TOML key (`postgres` in this example) must match a service name declared in the active SSG's `Coastfile.shared_service_groups`. The SSG is the single source of truth for the image, ports, env, and volumes of that service.

## Forbidden Fields

With `from_group = true`, the following fields are rejected at parse time:

- `image`
- `ports`
- `env`
- `volumes`

These all live on the SSG side. A consumer cannot override them because multiple consumers point at the same running container. If any of these fields appear alongside `from_group = true`, `coast build` fails with:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports.
```

## Allowed Overrides

Two fields are still legal per consumer:

- `inject` -- the env-var or file name through which the connection string is exposed. Projects can expose the same SSG Postgres under different env-var names.
- `auto_create_db` -- whether Coast should create a per-instance database inside this service at `coast run` time. Overrides the SSG service's own `auto_create_db` value.

## Conflict Detection

Two blocks with the same name in a single Coastfile are already rejected today. That rule stays.

A block with `from_group = true` that references a name not in the active SSG fails at `coast run` time:

```text
error: shared service 'postgres' references the shared service group, but no service 'postgres' exists in ~/.coast/ssg/latest.
```

## Auto-start

`coast run` on a consumer auto-starts the SSG when it is not already running:

- SSG build exists, container not running -> daemon runs the equivalent of `coast ssg start` (or `run` if the container was never created), guarded by the SSG-wide mutex.
- No SSG build exists at all -> hard error:

```text
Project 'my-app' references shared service 'postgres' from the Shared Service Group, but no SSG build exists. Run `coast ssg build` in the directory containing your Coastfile.shared_service_groups.
```

- SSG already running -> no-op, `coast run` continues immediately.

Progress events `SsgStarting` and `SsgStarted` fire on the run stream so [Coastguard](../concepts_and_terminology/COASTGUARD.md) can attribute the boot to the consumer project.

## How Routing Works

Inside a consumer Coast, app containers still believe their services live at canonical ports (`postgres:5432`). The daemon builds that illusion in two steps:

1. It resolves the consumer's `from_group = true` services against the SSG state DB and grabs their `(inner_port, dynamic_host_port)` pairs.
2. It spawns a docker0-alias-IP socat forwarder inside the Coast's DinD: `TCP-LISTEN:5432,bind=<alias>` -> `TCP:host.docker.internal:<dynamic>`, then adds `extra_hosts: {postgres: <alias>}` to the inner compose. The app container's DNS lookup for `postgres` resolves to the alias IP; the socat forwards to the SSG's published host port.

The inline-start path that the classic `[shared_services]` pattern uses is skipped for `from_group = true` services. No container is started on the host daemon -- the SSG is already running.

Net effect: app code and compose DNS do not change. Migrating a project from inline Postgres to SSG Postgres is a two-line Coastfile edit (remove image/ports/env, add `from_group = true`) plus a rebuild.

## Drift Detection

A consumer's `coast build` records the active SSG's state in the consumer's own `manifest.json`:

```json
{
  "ssg": {
    "build_id": "b455787d95cfdeb_20260420061903",
    "services": ["postgres", "redis"],
    "images": {
      "postgres": "postgres:16",
      "redis": "redis:7-alpine"
    }
  }
}
```

At `coast run` time, the daemon compares that snapshot against the active SSG's `latest` manifest:

- **Match.** `build_id`s are identical. Proceed silently.
- **Same-image warn.** `build_id`s differ but every referenced service still resolves to the same image. The daemon warns and proceeds:
  ```text
  SSG build differs (was b455787d95cfdeb_20260420061903, now 7812aa4...e6c2_20260421091255) but image refs still match for every referenced service. Proceeding.
  ```
- **Hard error.** An image ref changed for a referenced service, or a referenced service is missing from the active SSG:
  ```text
  SSG has changed since this coast was built. Re-run `coast build` to pick up the new SSG, or pin the SSG to the old build. (service 'postgres' image changed: postgres:15 -> postgres:16)
  ```

Drift always evaluates against the active SSG's `latest` build, not the currently-running one. Users who rebuilt the SSG but did not restart it still see the drift they introduced, so they cannot silently consume a stale SSG.

Pinning a consumer to an older SSG build (`coast ssg checkout-build <id>`) is tracked as a future enhancement. The current release requires `coast build` to pick up SSG changes.

## `auto_create_db`

`auto_create_db = true` on an SSG Postgres or MySQL service causes the daemon to create a `{instance}_{project}` database inside that service for every consumer Coast that runs. The database name mirrors what the existing inline `[shared_services]` pattern produces, so inject URLs agree with what `auto_create_db` produces out of the box. Naming details live in [Coastfile: Shared Services](../coastfiles/SHARED_SERVICES.md) and [DESIGN.md §13](../../coast-ssg/DESIGN.md#13-auto_create_db).

Creation is idempotent. Re-running `coast run` on an instance whose database already exists is a no-op. The underlying SQL is identical to the inline path, so DDL output is byte-for-byte the same regardless of which pattern your project uses.

A consumer can override the SSG service's `auto_create_db` value:

```toml
# SSG: auto_create_db = true, but this project doesn't want per-instance DBs.
[shared_services.postgres]
from_group = true
auto_create_db = false
```

## `inject`

`inject` exposes a connection string to the app container. Same format as [Secrets](../coastfiles/SECRETS.md): `"env:NAME"` creates an environment variable, `"file:/path"` writes a file.

The resolved string uses the canonical service name and port, not the dynamic host port. That invariance is the whole point -- app containers always see `postgres://coast:coast@postgres:5432/{db}` regardless of what dynamic port the SSG happens to be publishing on.

Phase 5 wires up `env:NAME` fully end to end. `file:/path` is recognized by the parser but the runtime does not yet write the file; expect that to land in a later release.

## Remote Coasts

Remote Coasts consume a local SSG through the pre-existing `SharedServicePortForward` protocol. The local daemon establishes a reverse SSH tunnel (`ssh -R`) from the remote machine back to the local SSG's dynamic host port. Inside the remote DinD, `extra_hosts: postgres: host-gateway` resolves `postgres` to the remote's host-gateway IP, and the SSH tunnel puts the local SSG on the other side.

The remote side (`coast-service`) stays SSG-agnostic. It speaks the same daemon-agnostic `shared_service_ports: Vec<SharedServicePortForward>` vocabulary it has always spoken; the local daemon simply puts different numbers on the wire.

Practical consequences:

- `coast ssg stop` / `rm` refuse while a remote shadow Coast is currently consuming the SSG. The daemon lists the blocking shadows so you know what is using the SSG.
- `coast ssg stop --force` (or `rm --force`) tears down the reverse-tunnel `ssh` children first, then proceeds. Use this when you accept that remote consumers will lose connectivity.
- SSH tunnel rewriting on the local daemon means you cannot point a remote Coast at an SSG that is already bound to canonical ports with `coast ssg checkout`. Checkout is a host-side convenience for the local user; remote consumers always go through the dynamic port.

See [Remote Coasts](../remote_coasts/README.md) for the broader architecture.

## See Also

- [Coastfile: Shared Services](../coastfiles/SHARED_SERVICES.md) -- full `[shared_services.*]` schema including `from_group = true`
- [Lifecycle](LIFECYCLE.md) -- what `coast run` does behind the scenes, including auto-start
- [Checkout](CHECKOUT.md) -- host-side canonical-port binding for ad-hoc tools
- [Volumes](VOLUMES.md) -- mounts and permissions; relevant when you rebuild the SSG and the new Postgres image changes data directory ownership
