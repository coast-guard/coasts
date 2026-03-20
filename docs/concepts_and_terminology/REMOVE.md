# Remove

`coast rm` tears down a Coast instance completely. It stops the instance if it
is running, removes the DinD container, deletes isolated volumes, deallocates
ports, removes agent shells, and deletes the instance from state.

```bash
coast rm dev-1
```

Most day-to-day workflows do not need `coast rm`. If you just want a Coast to
run different code or own the canonical ports, use [Assign and
Unassign](ASSIGN.md) or [Checkout](CHECKOUT.md) instead. Reach for `coast rm`
when you want to take Coasts down, reclaim per-instance runtime state, or
recreate an instance from scratch after rebuilding your Coastfile or build.

## What happens

`coast rm` executes five phases:

1. **Validate and locate** — looks up the instance in state. If the state
   record is gone but a dangling container with the expected name still exists,
   `coast rm` cleans that up too.
2. **Stop if needed** — if the instance is `Running` or `CheckedOut`, Coast
   brings the inner compose stack down and stops the DinD container first.
3. **Remove runtime artifacts** — removes the Coast container and deletes
   isolated volumes for that instance.
4. **Clean up host state** — kills lingering port forwarders, deallocates
   ports, removes agent shells, and deletes the instance record from the state
   database.
5. **Preserve shared data** — shared service volumes and shared service data are
   left alone.

## CLI usage

```text
coast rm <name>
coast rm --all
```

| Flag | Description |
|------|-------------|
| `<name>` | Remove one instance by name |
| `--all` | Remove every instance for the current project |

`coast rm --all` resolves the current project, lists its instances, and removes
them one by one. If there are no instances, it exits cleanly.

## Shared services and builds

- `coast rm` does **not** delete shared service data.
- Use `coast shared-services rm <service>` if you also want to remove a shared
  service and its data.
- Use `coast rm-build` if you want to remove build artifacts after taking
  instances down.

## When to use it

- after rebuilding your Coastfile or creating a new build and wanting a fresh
  instance
- when you want to take Coasts down and free per-instance container and volume
  state
- when an instance is wedged and starting fresh is easier than debugging it in
  place

## See also

- [Run](RUN.md) — creating a new Coast instance
- [Assign and Unassign](ASSIGN.md) — repointing an existing instance to a
  different worktree
- [Shared Services](SHARED_SERVICES.md) — what `coast rm` does not delete
- [Builds](BUILDS.md) — build artifacts and `coast rm-build`
