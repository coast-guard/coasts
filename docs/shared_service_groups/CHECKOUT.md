# SSG Host-Side Checkout

Consumer Coasts reach SSG services through the daemon's routing layer (in-DinD socat -> host socat -> dynamic port). That works great for app containers. It does not help host-side callers -- MCPs, ad-hoc `psql` sessions, your editor's database inspector -- that want to connect to `localhost:5432` as if the service lived right there.

`coast ssg checkout` solves that. It spawns a host-level socat that binds the canonical host port (5432 for Postgres, 6379 for Redis, ...) and forwards to the project's stable virtual port. From there the host's existing virtual-port socat carries the traffic on into the SSG's currently-published dynamic port.

The whole thing is per project. `coast ssg checkout --service postgres` resolves to the project that owns the cwd `Coastfile`; if you have two projects on this machine, only one can hold canonical port 5432 at a time.

## Usage

```bash
coast ssg checkout --service postgres     # bind one service
coast ssg checkout --all                  # bind every SSG service
coast ssg uncheckout --service postgres   # tear down one
coast ssg uncheckout --all                # tear down every active checkout
```

After a successful checkout, `coast ssg ports` annotates each bound service with `(checked out)`:

```text
  SERVICE              CANONICAL       DYNAMIC         VIRTUAL    STATUS
  postgres             5432            54201           42000      (checked out)
  redis                6379            54202           42001
```

Consumer Coasts always reach SSG services via their in-DinD socat -> virtual port chain, regardless of host-side checkout state. Checkout is purely a host-side convenience.

## Two-Hop Forwarder

The checkout socat does **not** point directly at the SSG's dynamic host port. It points at the project's stable virtual port:

```text
host process            -> 127.0.0.1:5432           (checkout socat, listens here)
                        -> 127.0.0.1:42000          (project's virtual port)
                        -> 127.0.0.1:54201          (SSG's current dynamic port)
                        -> <project>-ssg postgres   (inner service)
```

The two-hop chain means the checkout socat keeps working across SSG rebuilds even though the dynamic port shifts. Only the host's virtual-port socat updates -- the canonical-port socat is unaware. See [Routing](ROUTING.md) for how the host socat layer is maintained.

## Displacement of Coast-Instance Holders

When you ask the SSG to check out a canonical port, that port might already be held. The semantics depend on who holds it:

- **A Coast instance that was explicitly checked out.** `coast checkout <instance>` on some Coast earlier today bound `localhost:5432` to that Coast's inner Postgres. The SSG checkout **displaces** it: the daemon kills the existing socat, clears `port_allocations.socat_pid` for that Coast, and binds the SSG's socat instead. The CLI prints a clear warning:

  ```text
  Warning: displaced coast 'my-app/dev-2' from canonical port 5432.
  SSG checkout complete: postgres on canonical 5432 -> virtual 42000.
  ```

  The displaced Coast is **not** automatically rebound when you later `coast ssg uncheckout`. Its dynamic port still works, but the canonical port stays unbound until you run `coast checkout my-app/dev-2` again.

- **Another project's SSG checkout.** If `filemap-ssg` already has 5432 checked out and you try to check out `cg-ssg`'s 5432, the daemon refuses with a clear message naming the holder. Uncheckout `filemap-ssg`'s 5432 first.

- **A previous SSG checkout row with a dead `socat_pid`.** Stale metadata from a daemon that crashed or a stop/start cycle. The new checkout silently reclaims the row.

- **Anything else** (a host Postgres you started by hand, another daemon, `nginx` on port 8080). `coast ssg checkout` errors out:

  ```text
  error: port 5432 is held by a process outside Coast's tracking. Free the port with `lsof -i :5432` / `kill <pid>` and retry.
  ```

  There is no `--force` flag. Silently killing an unknown process was judged too dangerous.

## Stop / Start Behavior

`coast ssg stop` kills the live canonical-port socat processes but **preserves the checkout rows themselves** in the state DB.

`coast ssg run` / `start` / `restart` iterate the preserved rows and respawn a fresh canonical-port socat per row. The canonical port (5432) stays identical; only the dynamic port shifts between `run` cycles, and because the checkout socat targets the **virtual** port (which is also stable), the rebind is mechanical.

If a service disappears from the rebuilt SSG, its checkout row is dropped with a warning in the run response:

```text
SSG run complete. Dropped stale checkout for service 'mongo' (no longer in the active SSG build).
```

`coast ssg rm` wipes every `ssg_port_checkouts` row for the project. `rm` is destructive by design -- you explicitly asked for a clean slate.

## Daemon Restart Recovery

After an unexpected daemon restart (crash, `coastd restart`, reboot), `restore_running_state` consults the `ssg_port_checkouts` table and respawns every row against the current dynamic / virtual port allocation. Your `localhost:5432` stays bound across daemon churn.

## When to Check Out

- You want to point a GUI database client at the project's SSG Postgres.
- You want `psql "postgres://coast:coast@localhost:5432/mydb"` to work without discovering the dynamic port first.
- An MCP on your host needs a stable canonical endpoint.
- Coastguard wants to proxy the SSG's HTTP admin port.

When to **not** check out:

- For connectivity from inside a consumer Coast -- that already works via in-DinD socat to virtual port.
- When you're happy using `coast ssg ports` output and plugging the dynamic port into your tool.

## See Also

- [Routing](ROUTING.md) -- the canonical / dynamic / virtual port concepts and the full host-side forwarder chain
- [Lifecycle](LIFECYCLE.md) -- stop / start / rm details
- [Coast Checkout](../concepts_and_terminology/CHECKOUT.md) -- the Coast-instance version of this idea
- [Ports](../concepts_and_terminology/PORTS.md) -- canonical vs dynamic port plumbing across the whole system
