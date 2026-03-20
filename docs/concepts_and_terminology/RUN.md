# Run

`coast run` creates a new Coast instance. It resolves the latest [build](BUILDS.md), provisions a [DinD container](RUNTIMES_AND_SERVICES.md), loads cached images, starts your compose services, allocates [dynamic ports](PORTS.md), and records the instance in the state database.

```bash
coast run dev-1
```

If you pass `-w`, Coast also [assigns](ASSIGN.md) the worktree after provisioning completes:

```bash
coast run dev-1 -w feature/oauth
```

This is the most common pattern when a harness or agent creates a worktree and needs a Coast for it in one step.

## What happens

`coast run` executes four phases:

1. **Validate and insert** — checks the name is unique, resolves the build ID (from the `latest` symlink or an explicit `--build-id`), and inserts a `Provisioning` instance record.
2. **Docker provisioning** — creates the DinD container on the host daemon, builds any per-instance images, loads cached image tarballs into the inner daemon, rewrites the compose file, injects secrets, and runs `docker compose up -d`.
3. **Finalize** — stores port allocations, sets the primary port if there is exactly one, and transitions the instance to `Running`.
4. **Optional worktree assignment** — if `-w <worktree>` was provided, runs `coast assign` against the new instance. If the assignment fails, the Coast is still running — the failure is logged as a warning.

The persistent `/var/lib/docker` volume inside the DinD container means subsequent runs skip image loading. A fresh `coast run` with cold caches can take 20+ seconds; a re-run after `coast rm` typically finishes in under 10 seconds.

## CLI usage

```text
coast run <name> [options]
```

| Flag | Description |
|------|-------------|
| `-w`, `--worktree <name>` | Assign this worktree after provisioning completes |
| `--n <count>` | Batch creation. Name must contain `{n}` (e.g. `coast run dev-{n} --n=5` creates dev-1 through dev-5) |
| `-t`, `--type <type>` | Use a typed build (e.g. `--type snap` resolves `latest-snap` instead of `latest`) |
| `--force-remove-dangling` | Remove a leftover Docker container with the same name before creating |
| `-s`, `--silent` | Suppress progress output; only print the final summary or errors |
| `-v`, `--verbose` | Show verbose detail including Docker build logs |

The git branch is always auto-detected from the current HEAD.

## Batch creation

Use `{n}` in the name and `--n` to create multiple instances at once:

```bash
coast run dev-{n} --n=5
```

This creates `dev-1`, `dev-2`, `dev-3`, `dev-4`, `dev-5` sequentially. Each instance gets its own DinD container, port allocations, and volume state. Batches larger than 10 prompt for confirmation.

## Typed builds

If your project uses multiple Coastfile types (see [Coastfile Types](COASTFILE_TYPES.md)), pass `--type` to select which build to use:

```bash
coast run dev-1                    # resolves "latest"
coast run test-1 --type test       # resolves "latest-test"
coast run snapshot-1 --type snap   # resolves "latest-snap"
```

## Run vs assign and remove

- `coast run` creates a **new** instance. Use it when you need another Coast.
- `coast assign` repoints an **existing** instance to a different worktree. Use
  it when you already have a Coast and want to switch what code it runs.
- `coast rm` tears an instance down completely. Use it when you want to take
  Coasts down or recreate one from scratch.

Most day-to-day switching does not need `coast rm`; `coast assign` and
`coast checkout` are usually enough. Reach for `coast rm` when you want a clean
recreate, especially after rebuilding your Coastfile or build.

You can combine them: `coast run dev-3 -w feature/billing` creates the instance
and assigns the worktree in one step.

## Dangling containers

If a previous `coast run` was interrupted or `coast rm` did not fully clean up, you may see a "dangling Docker container" error. Pass `--force-remove-dangling` to remove the leftover container and proceed:

```bash
coast run dev-1 --force-remove-dangling
```

## See also

- [Remove](REMOVE.md) — tearing an instance down completely
- [Builds](BUILDS.md) — what `coast run` consumes
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) — the DinD architecture inside each instance
- [Assign and Unassign](ASSIGN.md) — switching an existing instance to a different worktree
- [Ports](PORTS.md) — how dynamic and canonical ports are allocated
- [Coasts](COASTS.md) — the high-level concept of a Coast instance
