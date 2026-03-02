# Troubleshooting

Most issues with Coasts come from stale state, orphaned Docker resources, or a daemon that got out of sync. This page covers the escalation path from mild to nuclear.

## Doctor

If things feel off — instances show as running but nothing responds, ports seem stuck, or the UI shows stale data — start with `coast doctor`:

```bash
coast doctor
```

Doctor scans the state database and Docker for inconsistencies: orphaned instance records with missing containers, dangling containers with no state record, and shared services marked running that are actually dead. It fixes what it finds automatically.

To preview what it would do without changing anything:

```bash
coast doctor --dry-run
```

## Daemon Restart

If the daemon itself seems unresponsive or you suspect it is in a bad state, restart it:

```bash
coast daemon restart
```

This sends a graceful shutdown signal, waits for the daemon to exit, and starts a fresh process. Your instances and state are preserved.

## Removing a Single Project

If the problem is isolated to one project, you can remove its build artifacts and associated Docker resources without affecting anything else:

```bash
coast rm-build my-project
```

This deletes the project's artifact directory, Docker images, volumes, and containers. It asks for confirmation first. Pass `--force` to skip the prompt.

## Factory Reset with Nuke

When nothing else works — or you just want a completely clean slate — `coast nuke` performs a full factory reset:

```bash
coast nuke
```

This will:

1. Stop the `coastd` daemon.
2. Remove **all** coast-managed Docker containers.
3. Remove **all** coast-managed Docker volumes.
4. Remove **all** coast-managed Docker networks.
5. Remove **all** coast Docker images.
6. Delete the entire `~/.coast/` directory (state database, builds, logs, secrets, image cache).
7. Recreate `~/.coast/` and restart the daemon so coast is immediately usable again.

Because this destroys everything, you must type `nuke` at the confirmation prompt:

```text
$ coast nuke
WARNING: This will permanently destroy ALL coast data:

  - Stop the coastd daemon
  - Remove all coast-managed Docker containers
  - Remove all coast-managed Docker volumes
  - Remove all coast-managed Docker networks
  - Remove all coast Docker images
  - Delete ~/.coast/ (state DB, builds, logs, secrets, image cache)

Type "nuke" to confirm:
```

Pass `--force` to skip the prompt (useful in scripts):

```bash
coast nuke --force
```

After a nuke, coast is ready to use — the daemon is running and the home directory exists. You just need to `coast build` and `coast run` your projects again.

## Reporting Bugs

If you hit a problem that is not resolved by any of the above, include the daemon logs when reporting:

```bash
coast daemon logs
```
