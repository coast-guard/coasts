# `coast ssg` CLI Reference

Every `coast ssg` subcommand talks to the same local daemon over the existing Unix socket. `coast shared-service-group` is an alias for `coast ssg`.

All commands accept a global `--silent` / `-s` flag that suppresses progress output and prints only the final summary or errors.

## Commands

| Command | Summary |
|---------|---------|
| `coast ssg build [-f <file>] [--working-dir <dir>] [--config '<toml>']` | Parse `Coastfile.shared_service_groups`, pull images, write artifact, flip `latest`, prune old builds. See [Building](BUILDING.md). |
| `coast ssg ps` | Show the current SSG build's service list (reads `manifest.json`, no container inspection). See [Building](BUILDING.md#inspecting-a-build-without-running-it). |
| `coast ssg run` | Create the singleton DinD and start all services. Allocates dynamic host ports. Streams progress events. See [Lifecycle](LIFECYCLE.md#coast-ssg-run). |
| `coast ssg start` | Start a previously-created but stopped SSG. Re-spawns any preserved checkout socats. See [Lifecycle](LIFECYCLE.md#coast-ssg-start). |
| `coast ssg stop [--force]` | Stop the SSG DinD. Preserves container and dynamic port mappings. `--force` tears down reverse SSH tunnels for remote shadow consumers first. See [Lifecycle](LIFECYCLE.md#coast-ssg-stop). |
| `coast ssg restart` | Stop + start. Preserves the container and dynamic ports. See [Lifecycle](LIFECYCLE.md#coast-ssg-restart). |
| `coast ssg rm [--with-data] [--force]` | Remove the SSG DinD. `--with-data` also drops inner named volumes. `--force` proceeds despite remote shadow consumers. Host bind mount contents are never touched. See [Lifecycle](LIFECYCLE.md#coast-ssg-rm). |
| `coast ssg logs [--service <name>] [--tail N] [--follow]` | Stream logs from the outer DinD or one inner service. `--follow` streams until Ctrl+C. See [Lifecycle](LIFECYCLE.md#coast-ssg-logs). |
| `coast ssg exec [--service <name>] -- <cmd...>` | Exec into the outer DinD or one inner service. Everything after `--` is passed through verbatim. See [Lifecycle](LIFECYCLE.md#coast-ssg-exec). |
| `coast ssg ports` | Show per-service canonical-to-dynamic mapping with `(checked out)` annotation where applicable. See [Checkout](CHECKOUT.md). |
| `coast ssg checkout [--service <name> \| --all]` | Bind canonical host ports via socat. Displaces Coast-instance holders with a warning; errors on unknown processes. See [Checkout](CHECKOUT.md). |
| `coast ssg uncheckout [--service <name> \| --all]` | Tear down canonical-port socats. Does not auto-restore displaced Coasts. See [Checkout](CHECKOUT.md). |
| `coast ssg doctor` | Read-only permission check on host bind mounts of known-image services. Emits ok/warn/info findings with `chown` remediation. See [Volumes -> coast ssg doctor](VOLUMES.md#coast-ssg-doctor). |

## Exit Codes

- `0` -- success. Commands like `doctor` return 0 even when they find warnings; they are diagnostic tools, not gates.
- Non-zero -- validation error, Docker error, state inconsistency, or remote-shadow gate refusal.

## See Also

- [Building](BUILDING.md)
- [Lifecycle](LIFECYCLE.md)
- [Volumes](VOLUMES.md)
- [Consuming](CONSUMING.md)
- [Checkout](CHECKOUT.md)
