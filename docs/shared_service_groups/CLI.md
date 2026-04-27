# `coast ssg` CLI Reference

Every `coast ssg` subcommand talks to the same local daemon over the existing Unix socket. `coast shared-service-group` is an alias for `coast ssg`.

Most verbs resolve a project from the cwd `Coastfile`'s `[coast].name` (or `--working-dir <dir>`). Only `coast ssg ls` is cross-project.

All commands accept a global `--silent` / `-s` flag that suppresses progress output and prints only the final summary or errors.

## Commands

### Build & inspect

| Command | Summary |
|---------|---------|
| `coast ssg build [-f <file>] [--working-dir <dir>] [--config '<toml>']` | Parse `Coastfile.shared_service_groups`, extract any `[secrets.*]`, pull images, write artifact at `~/.coast/ssg/<project>/builds/<id>/`, update `latest_build_id`, prune old builds. See [Building](BUILDING.md). |
| `coast ssg ps` | Show this project's SSG build's service list (reads `manifest.json` plus live container state). See [Lifecycle -> ps](LIFECYCLE.md#coast-ssg-ps). |
| `coast ssg builds-ls [--working-dir <dir>] [-f <file>]` | List every build artifact under `~/.coast/ssg/<project>/builds/` with timestamp, service count, and `(latest)` / `(pinned)` annotations. |
| `coast ssg ls` | Cross-project listing of every SSG known to the daemon (project, status, build id, service count, created-at). See [Lifecycle -> ls](LIFECYCLE.md#coast-ssg-ls). |

### Lifecycle

| Command | Summary |
|---------|---------|
| `coast ssg run` | Create the `<project>-ssg` DinD, allocate dynamic host ports, materialize secrets (when declared), boot the inner compose stack. See [Lifecycle -> run](LIFECYCLE.md#coast-ssg-run). |
| `coast ssg start` | Start a previously-created but stopped SSG. Re-materializes secrets and re-spawns any preserved canonical-port checkout socats. |
| `coast ssg stop [--force]` | Stop the project's SSG DinD. Preserves the container, dynamic ports, virtual ports, and checkout rows. `--force` tears down remote SSH tunnels first. |
| `coast ssg restart` | Stop + start. Preserves the container and dynamic ports. |
| `coast ssg rm [--with-data] [--force]` | Remove the project's SSG DinD. `--with-data` drops inner named volumes. `--force` proceeds despite remote shadow consumers. Host bind-mount contents are never touched. **Keystore is never touched** -- use `coast ssg secrets clear` for that. |

### Logs & exec

| Command | Summary |
|---------|---------|
| `coast ssg logs [--service <name>] [--tail N] [--follow]` | Stream logs from the outer DinD or one inner service. `--follow` streams until Ctrl+C. |
| `coast ssg exec [--service <name>] -- <cmd...>` | Exec into the outer `<project>-ssg` container or one inner service. Everything after `--` is passed through verbatim. |

### Routing & checkout

| Command | Summary |
|---------|---------|
| `coast ssg ports` | Show per-service canonical / dynamic / virtual port mapping with `(checked out)` annotation where applicable. See [Routing](ROUTING.md). |
| `coast ssg checkout [--service <name> \| --all]` | Bind canonical host ports via host-side socat (forwarder targets the project's stable virtual port). Displaces Coast-instance holders with a warning; errors on unknown host processes. See [Checkout](CHECKOUT.md). |
| `coast ssg uncheckout [--service <name> \| --all]` | Tear down canonical-port socats for this project. Does not auto-restore displaced Coasts. |

### Diagnostics

| Command | Summary |
|---------|---------|
| `coast ssg doctor` | Read-only check across host bind-mount permissions for known-image services and declared-but-unextracted SSG secrets. Emits `ok` / `warn` / `info` findings. See [Volumes -> coast ssg doctor](VOLUMES.md#coast-ssg-doctor). |

### Build pinning

| Command | Summary |
|---------|---------|
| `coast ssg checkout-build <BUILD_ID> [--working-dir <dir>] [-f <file>]` | Pin this project's SSG to a specific `build_id`. `coast ssg run` and `coast build` use the pin instead of `latest_build_id`. See [Building -> Locking a project to a specific build](BUILDING.md#locking-a-project-to-a-specific-build). |
| `coast ssg uncheckout-build [--working-dir <dir>] [-f <file>]` | Release the pin. Idempotent. |
| `coast ssg show-pin [--working-dir <dir>] [-f <file>]` | Show the current pin for this project, if any. |

### SSG-native secrets

| Command | Summary |
|---------|---------|
| `coast ssg secrets clear` | Drop every encrypted keystore entry under `coast_image = "ssg:<project>"`. Idempotent. The only verb that wipes SSG-native secrets -- `coast ssg rm` and `rm --with-data` deliberately leave them alone. See [Secrets](SECRETS.md). |

### Migration helper

| Command | Summary |
|---------|---------|
| `coast ssg import-host-volume <VOLUME> --service <name> --mount <path> [--apply] [-f <file>] [--working-dir <dir>] [--config '<toml>']` | Resolve a host Docker named volume's mountpoint and emit (or apply) the equivalent SSG bind-mount entry. See [Volumes -> coast ssg import-host-volume](VOLUMES.md#automating-the-recipe-coast-ssg-import-host-volume). |

## Exit Codes

- `0` -- success. Commands like `doctor` return 0 even when they find warnings; they are diagnostic tools, not gates.
- Non-zero -- validation error, Docker error, state inconsistency, or remote-shadow gate refusal.

## See Also

- [Building](BUILDING.md)
- [Lifecycle](LIFECYCLE.md)
- [Routing](ROUTING.md)
- [Volumes](VOLUMES.md)
- [Consuming](CONSUMING.md)
- [Secrets](SECRETS.md)
- [Checkout](CHECKOUT.md)
