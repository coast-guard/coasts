# Consuming a Shared Service Group

A consumer Coast opts into its project's SSG-owned services per service, using a one-line flag in the consumer's `Coastfile`. Inside the Coast, app containers still see `postgres:5432`; the daemon's routing layer redirects that traffic into the project's `<project>-ssg` outer DinD via a stable virtual port.

The SSG that `from_group = true` references is **always the consumer project's own SSG**. There is no cross-project sharing. If the consumer's `[coast].name` is `cg`, `from_group = true` resolves against `cg-ssg`'s `Coastfile.shared_service_groups`.

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

The TOML key (`postgres` in this example) must match a service name declared in the project's `Coastfile.shared_service_groups`.

## Forbidden Fields

With `from_group = true`, the following fields are rejected at parse time:

- `image`
- `ports`
- `env`
- `volumes`

These all live on the SSG side. If any appear alongside `from_group = true`, `coast build` fails with:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports, env, volumes.
```

## Allowed Overrides

Two fields are still legal per consumer:

- `inject` -- the env-var or file path through which the connection string is exposed. Different consumer projects can expose the same shape under different env-var names.
- `auto_create_db` -- whether Coast should create a per-instance database inside this service at `coast run` time. Overrides the SSG service's own `auto_create_db` value.

## Conflict Detection

Two `[shared_services.<name>]` blocks with the same name in a single Coastfile are rejected at parse time. That rule stays.

A block with `from_group = true` that references a name not declared in the project's `Coastfile.shared_service_groups` fails at `coast build` time:

```text
error: shared service 'postgres' has from_group = true but no service named 'postgres' is declared in Coastfile.shared_service_groups for project 'my-app'.
```

This is the typo check. There is no separate runtime "drift" check -- shape mismatches between consumer and SSG manifest at the build-time check, and any further mismatch at run time surfaces naturally as a connection error from the app's perspective.

## Auto-start

`coast run` on a consumer auto-starts the project's SSG when it isn't already running:

- SSG build exists, container not running -> daemon runs the equivalent of `coast ssg start` (or `run` if the container was never created), guarded by the project's SSG mutex.
- No SSG build exists at all -> hard error:

  ```text
  Project 'my-app' references shared service 'postgres' from the Shared Service Group, but no SSG build exists. Run `coast ssg build` in the directory containing your Coastfile.shared_service_groups.
  ```

- SSG already running -> no-op, `coast run` continues immediately.

Progress events `SsgStarting` and `SsgStarted` fire on the run stream so [Coastguard](../concepts_and_terminology/COASTGUARD.md) can attribute the boot to the consumer project.

## How Routing Works

Inside a consumer Coast, the app container resolves `postgres:5432` to the project's SSG via three pieces:

1. **Alias IP + `extra_hosts`** add `postgres -> <docker0 alias IP>` to the consumer's inner compose, so DNS lookups for `postgres` succeed.
2. **In-DinD socat** listens on `<alias>:5432` and forwards to `host.docker.internal:<virtual_port>`. The virtual port is stable for `(project, service, container_port)` -- it doesn't change when the SSG is rebuilt.
3. **Host socat** on `<virtual_port>` forwards to `127.0.0.1:<dynamic>`, where `<dynamic>` is the SSG container's currently-published port. The host socat updates when the SSG is rebuilt; the consumer's in-DinD socat never has to change.

App code and compose DNS don't change. Migrating a project from inline Postgres to SSG Postgres is a small Coastfile edit (remove `image`/`ports`/`env`, add `from_group = true`) plus a rebuild.

For the full hop-by-hop walkthrough, port concepts, and rationale, see [Routing](ROUTING.md).

## `auto_create_db`

`auto_create_db = true` on an SSG Postgres or MySQL service causes the daemon to create a `{instance}_{project}` database inside that service for every consumer Coast that runs. The database name matches what the inline `[shared_services]` pattern produces, so `inject` URLs agree with the database `auto_create_db` creates.

Creation is idempotent. Re-running `coast run` on an instance whose database already exists is a no-op. The underlying SQL is identical to the inline path, so DDL output is byte-for-byte the same regardless of which pattern your project uses.

A consumer can override the SSG service's `auto_create_db` value:

```toml
# SSG: auto_create_db = true, but this project doesn't want per-instance DBs.
[shared_services.postgres]
from_group = true
auto_create_db = false
```

## `inject`

`inject` exposes a connection string to the app container. Same format as [Secrets](../coastfiles/SECRETS.md): `"env:NAME"` creates an environment variable, `"file:/path"` writes a file inside the consumer's coast container and bind-mounts it read-only into every non-stubbed inner compose service.

The resolved string uses the canonical service name and canonical port, not the dynamic host port. That invariance is the whole point -- app containers always see `postgres://coast:coast@postgres:5432/{db}` regardless of what dynamic port the SSG happens to be publishing on.

Both `env:NAME` and `file:/path` are fully implemented.

This `inject` is the **consumer-side** secret pipeline: the value is computed from canonical SSG metadata at `coast build` time and injected into the consumer's coast DinD. It is independent of the **SSG-side** `[secrets.*]` pipeline (see [Secrets](SECRETS.md)) which extracts values for the SSG's *own* services to consume.

## Remote Coasts

A remote Coast (one created with `coast assign --remote ...`) reaches a local SSG through a reverse SSH tunnel. The local daemon spawns `ssh -N -R <vport>:localhost:<vport>` from the remote machine back to the local virtual port; inside the remote DinD, `extra_hosts: postgres: host-gateway` resolves `postgres` to the remote's host-gateway IP, and the SSH tunnel puts the local SSG on the other side at the same virtual port number.

Both sides of the tunnel use the **virtual** port, not the dynamic port. This means rebuilding the SSG locally never invalidates the remote tunnel.

Tunnels are coalesced per `(project, remote_host, service, container_port)` -- multiple consumer instances of the same project on the same remote share one `ssh -R` process. Removing one consumer doesn't tear down the tunnel; only the last consumer's removal does.

Practical consequences:

- `coast ssg stop` / `rm` refuse while a remote shadow Coast is currently consuming the SSG. The daemon lists the blocking shadows so you know what's using the SSG.
- `coast ssg stop --force` (or `rm --force`) tears down the shared `ssh -R` first, then proceeds. Use this when you accept that remote consumers will lose connectivity.

See [Routing](ROUTING.md) for the full remote-tunnel architecture and [Remote Coasts](../remote_coasts/README.md) for the broader remote-machine setup.

## See Also

- [Routing](ROUTING.md) -- canonical / dynamic / virtual port concepts and the full routing chain
- [Secrets](SECRETS.md) -- SSG-native `[secrets.*]` for service-side credentials (orthogonal to consumer-side `inject`)
- [Coastfile: Shared Services](../coastfiles/SHARED_SERVICES.md) -- full `[shared_services.*]` schema including `from_group = true`
- [Lifecycle](LIFECYCLE.md) -- what `coast run` does behind the scenes, including auto-start
- [Checkout](CHECKOUT.md) -- host-side canonical-port binding for ad-hoc tools
- [Volumes](VOLUMES.md) -- mounts and permissions; relevant when you rebuild the SSG and the new Postgres image changes data directory ownership
