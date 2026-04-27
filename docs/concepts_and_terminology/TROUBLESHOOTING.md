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

## SSG Permission Mismatches

If a [Shared Service Group](../shared_service_groups/README.md) Postgres (or MySQL / MariaDB / MongoDB) exits at startup with a "data directory has wrong ownership" error, the host bind-mount directory was created with the wrong UID/GID. Run:

```bash
coast ssg doctor
```

It reports one finding per `(service, host-bind-mount)` pair and includes the exact `chown` command to fix any mismatch. Doctor is read-only -- it never modifies the directory itself. See [SSG Volumes](../shared_service_groups/VOLUMES.md#coast-ssg-doctor) for the full semantics.

## Missing Shared Service Images

If `coast run` fails while creating a shared service with an error like `No such image: postgres:15`, the image is missing from your host Docker daemon.

This most commonly happens when your `Coastfile` defines `shared_services` such as Postgres or Redis and Docker has not pulled those images yet.

Pull the missing image, then run the instance again:

```bash
docker pull postgres:15
docker pull redis:7
coast run my-instance
```

If you are not sure which image is missing, the failing `coast run` output will include the image name in the Docker error. After a failed provisioning attempt, Coasts cleans up the partial instance automatically, so seeing the instance return to `stopped` is expected.

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
