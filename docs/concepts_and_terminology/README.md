# Concepts and Terminology

This section covers the core concepts and vocabulary used throughout Coasts. If you are new to Coasts, start here before diving into configuration or advanced usage.

- [Coasts](COASTS.md) — self-contained runtimes of your project, each with its own ports, volumes, and worktree assignment.
- [Run](RUN.md) — creating a new Coast instance from the latest build, optionally assigning a worktree.
- [Remove](REMOVE.md) — tearing down a Coast instance and its isolated runtime state when you need a clean recreate or want to take Coasts down.
- [Filesystem](FILESYSTEM.md) — the shared mount between host and Coast, host-side agents, and worktree switching.
- [Coast Daemon](DAEMON.md) — the local `coastd` control plane that executes lifecycle operations.
- [Coast CLI](CLI.md) — the terminal interface for commands, scripts, and agent workflows.
- [Coastguard](COASTGUARD.md) — the web UI launched with `coast ui` for observability and control.
- [Ports](PORTS.md) — canonical ports vs dynamic ports and how checkout swaps between them.
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) — quick-links to your primary service, subdomain routing for cookie isolation, and URL templates.
- [Assign and Unassign](ASSIGN.md) — switching a Coast between worktrees and the available assign strategies.
- [Checkout](CHECKOUT.md) — mapping canonical ports to a Coast instance and when you need it.
- [Lookup](LOOKUP.md) — discovering which Coast instances match the agent's current worktree.
- [Volume Topology](VOLUMES.md) — shared services, shared volumes, isolated volumes, and snapshotting.
- [Shared Services](SHARED_SERVICES.md) — host-managed infrastructure services and volume disambiguation.
- [Secrets and Extractors](SECRETS.md) — extracting host secrets and injecting them into Coast containers.
- [Builds](BUILDS.md) — the anatomy of a coast build, where artifacts live, auto-pruning, and typed builds.
- [Coastfile Types](COASTFILE_TYPES.md) — composable Coastfile variants with extends, unset, omit, and autostart.
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) — the DinD runtime, Docker-in-Docker architecture, and how services run inside a Coast.
- [Bare Services](BARE_SERVICES.md) — running non-containerized processes inside a Coast and why you should containerize instead.
- [Logs](LOGS.md) — reading service logs from inside a Coast, the MCP tradeoff, and the Coastguard log viewer.
- [Exec & Docker](EXEC_AND_DOCKER.md) — running commands inside a Coast and talking to the inner Docker daemon.
- [Agent Shells](AGENT_SHELLS.md) — containerized agent TUIs, the OAuth tradeoff, and why you should probably run agents on the host instead.
- [MCP Servers](MCP_SERVERS.md) — configuring MCP tools inside a Coast for containerized agents, internal vs host-proxied servers.
- [Troubleshooting](TROUBLESHOOTING.md) — doctor, daemon restart, project removal, and the factory-reset nuke option.
