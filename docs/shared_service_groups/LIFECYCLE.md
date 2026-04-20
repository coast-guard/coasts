# SSG Lifecycle

The SSG lifecycle is modeled on the Coast lifecycle. One singleton container named `coast-ssg` holds every shared service as an inner Docker Compose stack. Every mutating command in this page serializes through a process-global `ssg_mutex` on the daemon, so two concurrent `coast ssg run` / `coast ssg stop` invocations cannot collide.

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

- `coast ssg build` does not create a container. It only produces an artifact on disk.
- `coast ssg run` creates the singleton DinD, allocates dynamic host ports, writes both into the state DB, and boots the inner compose stack.
- `coast ssg stop` stops the outer DinD but preserves the container and dynamic port rows so `start` is fast.
- `coast ssg start` re-spawns the SSG from the same artifact with the same dynamic ports.
- `coast ssg rm` removes the outer DinD container. With `--with-data` it also drops the inner named volumes (host bind mount contents are never touched).
- `coast ssg restart` is a convenience wrapper for `stop` + `start`.

## Commands

### `coast ssg run`

Creates the singleton DinD if it does not exist and starts its inner services. Allocates one dynamic host port per declared service and publishes them on the outer DinD. Writes the mappings into the state DB so the port allocator does not reuse them.

```bash
coast ssg run
```

Streams progress events via the same `BuildProgressEvent` channel as `coast ssg build`, so the CLI renders `[N/M]` step counters for image loads, compose boot, and port allocation.

**Auto-start**. `coast run` on a consumer Coast that references an SSG service auto-starts the SSG if it is not already running. You can always run `coast ssg run` explicitly, but you rarely need to. See [Consuming -> Auto-start](CONSUMING.md#auto-start).

### `coast ssg start`

Starts a previously-stopped SSG. Requires an existing `coast-ssg` container (i.e. a prior `coast ssg run`). Re-spawns host-side checkout socats for any canonical ports that were checked out before the stop.

```bash
coast ssg start
```

### `coast ssg stop`

Stops the outer DinD container. The inner compose stack goes down with it. The container and the dynamic port allocations are preserved in the state DB so the next `start` is fast.

```bash
coast ssg stop
coast ssg stop --force
```

Host-side checkout socats are killed but their rows in the state DB survive with `socat_pid = NULL`. The next `coast ssg start` or `coast ssg run` re-spawns them against the new dynamic ports. See [Checkout](CHECKOUT.md).

**Remote-consumer gate.** The daemon refuses to stop the SSG while any remote shadow Coast is currently consuming it. Pass `--force` to tear down the reverse SSH tunnels those remote Coasts are using and proceed anyway. See [Consuming -> Remote Coasts](CONSUMING.md#remote-coasts).

### `coast ssg restart`

Equivalent to `stop` + `start`. Preserves the container and dynamic port mappings.

```bash
coast ssg restart
```

### `coast ssg rm`

Removes the outer DinD container. By default this preserves inner named volumes (Postgres WAL, etc.), so your data survives across `rm` / `run` cycles. Host bind mount contents are never touched.

```bash
coast ssg rm
coast ssg rm --with-data
coast ssg rm --force
```

- `--with-data` also drops every inner named volume before removing the DinD itself. Use this when you genuinely want a clean slate.
- `--force` proceeds even when remote shadow Coasts reference the SSG. Same semantics as `stop --force`.
- `rm` clears every `ssg_port_checkouts` row (destructive -- you asked for a clean slate).

### `coast ssg ps`

Shows service status. Reads `manifest.json` for the built configuration, then inspects the live state DB for running-container metadata.

```bash
coast ssg ps
```

Output after a successful `run`:

```text
SSG build: b455787d95cfdeb_20260420061903  (running)

  SERVICE              IMAGE                          PORT       STATUS
  postgres             postgres:16                    5432       running
  redis                redis:7-alpine                 6379       running
```

### `coast ssg ports`

Shows the per-service canonical-to-dynamic port mapping, with a `(checked out)` annotation when a host-side canonical-port socat is live.

```bash
coast ssg ports

#   SERVICE              CANONICAL       DYNAMIC         STATUS
#   postgres             5432            54201           (checked out)
#   redis                6379            54202
```

### `coast ssg logs`

Streams logs from the outer DinD container or from a specific inner service.

```bash
coast ssg logs --tail 100
coast ssg logs --service postgres --tail 50
coast ssg logs --service postgres --follow
```

- `--service <name>` targets an inner service by compose key; without it you get the outer DinD's stdout.
- `--tail N` caps the number of historical lines (defaults to 200).
- `--follow` / `-f` streams new lines as they arrive, until you `Ctrl+C`.

### `coast ssg exec`

Executes a command inside the outer DinD or an inner service.

```bash
coast ssg exec -- sh
coast ssg exec --service postgres -- psql -U coast -l
```

- Without `--service`, the command runs in the outer DinD container.
- With `--service <name>`, the command runs inside that compose service via `docker compose exec -T`.
- Everything after `--` is passed through to the underlying `docker exec`, including flags.

## Mutex Semantics

Every mutating SSG verb (`run`/`start`/`stop`/`restart`/`rm`/`checkout`/`uncheckout`) acquires a process-global `ssg_mutex` inside the daemon before dispatching to the real handler. Two concurrent invocations queue rather than race. Read-only verbs (`ps`/`ports`/`logs`/`exec`/`doctor`) do not acquire the mutex.

## Coastguard Integration

If you are running [Coastguard](../concepts_and_terminology/COASTGUARD.md), the UI renders the SSG lifecycle alongside the projects panel. `CoastEvent::SsgStarting` and `CoastEvent::SsgStarted` fire whenever a consumer Coast triggers an auto-start, so the UI can attribute the boot to the project that needed it.
