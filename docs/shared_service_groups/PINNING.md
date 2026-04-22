# Pinning a consumer to a specific SSG build

> **Beta.** See [Shared Service Groups](README.md) for the feature-wide caveat.

When you rebuild the SSG (`coast ssg build`), the `~/.coast/ssg/latest` symlink moves to the new build. By default every consumer's drift check and auto-start path follows `latest`. That is almost always what you want -- one source of truth across every project.

A **pin** tells one consumer to evaluate against a specific SSG build instead of whatever `latest` currently points at. Pins are consumer-local, keyed by project name, and survive SSG churn.

## When to pin

- **Upgrade the SSG ahead of rebuilding every consumer.** Pin each consumer to the old build, ship the new SSG, then uncheckout project-by-project on your own schedule.
- **Roll back after a bad SSG build.** Pin any consumer that was stable on the previous build while you fix `Coastfile.shared_service_groups` and re-run `coast ssg build`.
- **Test a migration on one consumer.** Build a new SSG, pin only the project you are testing, watch for regressions, then promote or roll back.

Pins are **not** a general production pinning strategy. They are a local-developer escape hatch for SSG churn.

## Commands

### `coast ssg checkout-build <BUILD_ID>`

Pin the current project to a specific SSG build.

```bash
# See available build ids (newest last)
ls ~/.coast/ssg/builds/

# Pin this project to an older build
coast ssg checkout-build df5bddb5b7a39b11_20260422051132
```

Runs in the consumer's checkout. The project name comes from `[coast].name` in the local Coastfile. Pass `--project <name>` to override.

The build id is validated at pin time. A typo or a pruned build id fails fast with a message pointing you at `ls ~/.coast/ssg/builds/`.

### `coast ssg uncheckout-build`

Drop the pin for the current project.

```bash
coast ssg uncheckout-build
```

Idempotent. If no pin exists the command exits successfully with a `no-pin` message. After this, drift checks and auto-start follow `latest` again.

### `coast ssg show-pin`

Show the current pin (if any).

```bash
coast ssg show-pin
# Project 'my-consumer' is pinned to SSG build df5bddb5b7a39b11_20260422051132 (pinned at 2026-04-22T00:00:00Z).
```

All three commands accept `--project`, `--working-dir`, and `-f/--file` to point at a different consumer.

## What the pin affects

| Subsystem | Behavior with a pin |
|-----------|---------------------|
| Drift check | Compares the consumer's recorded `ssg.build_id` against the **pinned** manifest instead of the `latest` symlink. See [Consuming](CONSUMING.md#drift-detection). |
| Auto-start on `coast run` | When the SSG is not yet running, boots the **pinned** build id, not `latest`. See [Lifecycle](LIFECYCLE.md#auto-start-on-coast-run). |
| `coast ssg build` auto-prune | Preserves every pinned build id across rebuilds. A pinned build will not be pruned even if it would otherwise be the oldest entry past the retention count. |

## What the pin does NOT do

- **It does not lock the running SSG to your build.** The SSG is a singleton. If another consumer boots a newer build while yours is stopped, your next `coast run` sees the mismatch and the existing drift check fires. Pinning protects you from silent follow-through, not from an active SSG running a different build.
- **It does not transfer across machines.** Pins live in the daemon's SQLite state at `~/.coast/state.db`. A fresh machine starts with no pins.
- **It does not survive `~/.coast/state.db` being wiped.** If you rebuild state you also lose the pin record. The build directory under `~/.coast/ssg/builds/` is untouched, so re-run `coast ssg checkout-build <id>` to re-pin.

## Troubleshooting

### "pinned for this coast but no longer exists on disk"

The build directory under `~/.coast/ssg/builds/<id>` is gone. Two remedies:

1. Drop the pin and fall back to `latest`:
   ```bash
   coast ssg uncheckout-build
   ```
2. Rebuild the SSG from the Coastfile that produced the pinned id:
   ```bash
   coast ssg build
   ```

### "SSG has changed since this coast was built" with a pin set

The running SSG container is on a different build than your pin. Stop the SSG and re-run -- the auto-start path will boot your pinned build:

```bash
coast ssg stop
coast run <instance>
```

If another user / project needs the newer SSG to stay up, you need to either rebuild your own consumer against the newer SSG (then `uncheckout-build`), or coordinate so only one SSG build is active at a time.

### "no pin found for project"

You are in the wrong checkout or the project name does not match what the pin was created under. Confirm with:

```bash
coast ssg show-pin --project <name>
```

## See also

- [Consuming](CONSUMING.md) -- drift detection mechanics.
- [Lifecycle](LIFECYCLE.md) -- auto-start path.
- [Building](BUILDING.md) -- build id format and auto-prune retention.
