# SSG Lifecycle

Each project's SSG is its own outer Docker-in-Docker container named `<project>-ssg` (e.g. `cg-ssg`). Lifecycle verbs target the SSG of whichever project owns the cwd `Coastfile` (or the project named via `--working-dir`). Every mutating command serializes through a per-project mutex on the daemon, so two concurrent `coast ssg run` / `coast ssg stop` invocations against the same project queue rather than race -- but two different projects can mutate their SSGs in parallel.

## State Machine

```text
                     coast ssg build           coast ssg run
(no build)   -->  built     -->     created    -->     running
                                                          |
                                                   coast ssg stop
                                                          v
                                                       stopped
                                                          |
                                                  coast ssg start
                                                          v
                                                       running
                                                          |
                                                   coast ssg rm
                                                          v
                                                      (removed)
```

- `coast ssg build` does not create a container. It produces an artifact on disk under `~/.coast/ssg/<project>/builds/<id>/` and (when `[secrets.*]` is declared) extracts secret values into the keystore.
- `coast ssg run` creates the `<project>-ssg` DinD, allocates dynamic host ports, materializes any declared secrets into a per-run `compose.override.yml`, and boots the inner compose stack.
- `coast ssg stop` stops the outer DinD but preserves the container, the dynamic port rows, and the per-project virtual ports so `start` is fast.
- `coast ssg start` re-spawns the SSG and re-materializes secrets (so a `coast ssg secrets clear` between stop and start takes effect).
- `coast ssg rm` removes the outer DinD container. With `--with-data` it also drops the inner named volumes (host bind-mount contents are never touched). The keystore is never cleared by `rm` -- only `coast ssg secrets clear` does that.
- `coast ssg restart` is a convenience wrapper for `stop` + `start`.

## Commands

### `coast ssg run`

Creates the `<project>-ssg` DinD if it doesn't exist and starts its inner services. Allocates one dynamic host port per declared service and publishes them on the outer DinD. Writes the mappings into the state DB so the port allocator does not reuse them.

```bash
coast ssg run
```

Streams progress events via the same `BuildProgressEvent` channel as `coast ssg build`. The default plan has 7 steps:

1. Preparing SSG
2. Creating SSG container
3. Starting SSG container
4. Waiting for inner daemon
5. Loading cached images
6. Materializing secrets (silent when no `[secrets]` block; emits per-secret items otherwise)
7. Starting inner services

**Auto-start**. `coast run` on a consumer Coast that references an SSG service auto-starts the SSG if it isn't already running. You can always run `coast ssg run` explicitly, but you rarely need to. See [Consuming -> Auto-start](CONSUMING.md#auto-start).

### `coast ssg start`

Starts a previously-stopped SSG. Requires an existing `<project>-ssg` container (i.e. a prior `coast ssg run`). Re-materializes secrets from the keystore so any change since stop takes effect, then re-spawns host-side checkout socats for any canonical ports that were checked out before the stop.

```bash
coast ssg start
```

### `coast ssg stop`

Stops the outer DinD container. The inner compose stack goes down with it. The container, the dynamic port allocations, and the per-project virtual port rows are preserved so the next `start` is fast.

```bash
coast ssg stop
coast ssg stop --force
```

Host-side checkout socats are killed but their rows in the state DB survive. The next `coast ssg start` or `coast ssg run` re-spawns them. See [Checkout](CHECKOUT.md).

**Remote-consumer gate.** The daemon refuses to stop the SSG while any remote shadow Coast (one created with `coast assign --remote ...`) is currently consuming it. Pass `--force` to tear down the reverse SSH tunnels and proceed anyway. See [Consuming -> Remote Coasts](CONSUMING.md#remote-coasts).

### `coast ssg restart`

Equivalent to `stop` + `start`. Preserves the container and dynamic port mappings.

```bash
coast ssg restart
```

### `coast ssg rm`

Removes the outer DinD container. By default this preserves inner named volumes (Postgres WAL, etc.), so your data survives across `rm` / `run` cycles. Host bind-mount contents are never touched.

```bash
coast ssg rm                    # preserves named volumes; preserves keystore
coast ssg rm --with-data        # also drops named volumes; still preserves keystore
coast ssg rm --force            # proceeds despite remote consumers
```

- `--with-data` drops every inner named volume before removing the DinD itself. Use this when you want a fresh database.
- `--force` proceeds even when remote shadow Coasts reference the SSG. Same semantics as `stop --force`.
- `rm` clears `ssg_port_checkouts` rows (destructive on the canonical-port host bindings).

The keystore -- where SSG-native secrets live (`coast_image = "ssg:<project>"`) -- is **not** affected by `rm` or `rm --with-data`. To wipe SSG secrets, use `coast ssg secrets clear` (see [Secrets](SECRETS.md)).

### `coast ssg ps`

Shows service status for the current project's SSG. Reads `manifest.json` for the built configuration, then inspects the live state DB for running-container metadata.

```bash
coast ssg ps
```

Output after a successful `run`:

```text
SSG build: b455787d95cfdeb_20260420061903  (project: cg, running)

  SERVICE              IMAGE                          PORT       STATUS
  postgres             postgres:16                    5432       running
  redis                redis:7-alpine                 6379       running
```

### `coast ssg ports`

Shows the per-service canonical / dynamic / virtual port mapping, with a `(checked out)` annotation when a host-side canonical-port socat is live for that service. The virtual port is what consumers actually connect to. See [Routing](ROUTING.md) for details.

```bash
coast ssg ports

#   SERVICE              CANONICAL       DYNAMIC         VIRTUAL    STATUS
#   postgres             5432            54201           42000      (checked out)
#   redis                6379            54202           42001
```

### `coast ssg logs`

Streams logs from the outer DinD container or from a specific inner service.

```bash
coast ssg logs --tail 100
coast ssg logs --service postgres --tail 50
coast ssg logs --service postgres --follow
```

- `--service <name>` targets an inner service by compose key; without it you get the outer DinD's stdout.
- `--tail N` caps historical lines (default 200).
- `--follow` / `-f` streams new lines as they arrive, until `Ctrl+C`.

### `coast ssg exec`

Executes a command inside the outer DinD or an inner service.

```bash
coast ssg exec -- sh
coast ssg exec --service postgres -- psql -U coast -l
```

- Without `--service`, the command runs in the outer `<project>-ssg` container.
- With `--service <name>`, the command runs inside that compose service via `docker compose exec -T`.
- Everything after `--` is passed through to the underlying `docker exec`, including flags.

### `coast ssg ls`

Lists every SSG known to the daemon, across every project. This is the only verb that doesn't resolve a project from cwd; it returns rows for every entry in the daemon's SSG state.

```bash
coast ssg ls

#   PROJECT     STATUS     BUILD                                       SERVICES   CREATED
#   cg          running    b455787d95cfdeb_20260420061903               2          2026-04-20T06:19:03Z
#   filemap     stopped    b9b93fdb41b21337_20260418123012               3          2026-04-18T12:30:12Z
```

Useful for catching forgotten SSGs from old projects, or for quickly seeing which projects on this machine have an SSG in any state.

## Mutex Semantics

Every mutating SSG verb (`run`/`start`/`stop`/`restart`/`rm`/`checkout`/`uncheckout`) acquires a per-project SSG mutex inside the daemon before dispatching to the real handler. Two concurrent invocations against the same project queue; against different projects they run in parallel. Read-only verbs (`ps`/`ports`/`logs`/`exec`/`doctor`/`ls`) do not acquire the mutex.

## Coastguard Integration

If you are running [Coastguard](../concepts_and_terminology/COASTGUARD.md), the SPA renders the SSG lifecycle on its own page (`/project/<p>/ssg/local`) with tabs for Exec, Ports, Services, Logs, Secrets, Stats, Images, and Volumes. `CoastEvent::SsgStarting` and `CoastEvent::SsgStarted` fire whenever a consumer Coast triggers an auto-start, so the UI can attribute the boot to the project that needed it.
