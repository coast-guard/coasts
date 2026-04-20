# SSG Host-Side Checkout

Coast instances reach SSG services through an internal socat layer that forwards `postgres:5432` inside the Coast to the SSG's dynamic host port. That works great for app containers. It does not help host-side callers -- MCPs, ad-hoc `psql` sessions, Coastguard previews, your editor's database inspector -- that want to connect to `localhost:5432` as if the service lived right there.

`coast ssg checkout` solves that. It spawns a host-level socat that binds the canonical host port (5432 for Postgres, 6379 for Redis, ...) and forwards to the SSG's dynamic host port.

## Usage

```bash
coast ssg checkout --service postgres     # bind one service
coast ssg checkout --all                  # bind every SSG service
coast ssg uncheckout --service postgres   # tear down one
coast ssg uncheckout --all                # tear down every active checkout
```

After a successful checkout, `coast ssg ports` annotates each bound service with `(checked out)`:

```text
  SERVICE              CANONICAL       DYNAMIC         STATUS
  postgres             5432            54201           (checked out)
  redis                6379            54202
```

Coast instances always reach SSG services via the internal socat path, regardless of host-side checkout state. Checkout is purely a host-side convenience.

## Displacement of Coast-Instance Holders

When you ask the SSG to check out a canonical port, that port might already be held. The semantics depend on who holds it:

- **A Coast instance that was explicitly checked out.** `coast checkout <instance>` on some Coast earlier today bound `localhost:5432` to that Coast's inner Postgres. The SSG checkout **displaces** it: the daemon kills the existing socat, clears `port_allocations.socat_pid` for that Coast, and binds the SSG's socat instead. The CLI prints a clear warning:

  ```text
  Warning: displaced coast 'my-app/dev-2' from canonical port 5432.
  SSG checkout complete: postgres on canonical 5432.
  ```

  The displaced Coast is **not** automatically rebound when you later `coast ssg uncheckout`. Its dynamic port still works, but the canonical port stays unbound until you run `coast checkout my-app/dev-2` again.

- **A previous SSG checkout row with a dead `socat_pid`.** Stale metadata from a daemon that crashed or a stop/start cycle. The new checkout silently reclaims the row.

- **Anything else** (a host Postgres you started by hand, another daemon, `nginx` on port 8080). `coast ssg checkout` errors out:

  ```text
  error: port 5432 is held by a process outside Coast's tracking. Free the port with `lsof -i :5432` / `kill <pid>` and retry.
  ```

  There is no `--force` flag. Silently killing an unknown process was judged too dangerous. The remediation is one command away.

## Stop / Start Behavior

`coast ssg stop` does two things to the checkout state:

1. Kills the live socat processes (they would be forwarding to a now-dead dynamic port anyway).
2. Nulls `socat_pid` on each row but **preserves the rows themselves**.

`coast ssg run` / `start` / `restart` iterate the preserved rows and respawn a fresh socat per row against the **newly-allocated** dynamic ports. The inner port number (5432) stays identical; only the outer dynamic port shifts between `run` cycles, and the checkout socat is rebuilt against the new number.

If a service disappears from the rebuilt SSG, its checkout row is dropped with a warning in the run response:

```text
SSG run complete. Dropped stale checkout for service 'mongo' (no longer in the active SSG build).
```

`coast ssg rm` wipes every `ssg_port_checkouts` row. `rm` is destructive by design -- you explicitly asked for a clean slate.

## Daemon Restart Recovery

After an unexpected daemon restart (crash, `coastd restart`, reboot), `restore_running_state` consults the `ssg_port_checkouts` table and respawns every row against the current dynamic port allocation. Your `localhost:5432` stays bound across daemon churn.

## When to Check Out

- You want to point a GUI database client at the SSG Postgres.
- You want `psql "postgres://coast:coast@localhost:5432/mydb"` to work without discovering the dynamic port first.
- An MCP on your host needs a stable canonical endpoint.
- Coastguard wants to proxy the SSG's HTTP admin port.

When to **not** check out:

- For connectivity from inside a Coast -- that already works via the internal socat.
- When you are happy using `coast ssg ports` output and plugging the dynamic port into your tool.

## See Also

- [Lifecycle](LIFECYCLE.md) -- stop / start / rm details
- [Coast Checkout](../concepts_and_terminology/CHECKOUT.md) -- the Coast-instance version of this idea
- [Ports](../concepts_and_terminology/PORTS.md) -- canonical vs dynamic port plumbing across the whole system
