# Coast Daemon

The Coast daemon (`coastd`) is the long-running local process that does the actual orchestration work. The [CLI](CLI.md) and [Coastguard](COASTGUARD.md) are clients; `coastd` is the control plane behind them.

## Architecture at a Glance

```text
coast CLI (automation) -----+
                            +--> coastd daemon
Coastguard UI (human) ------+         |
                                      +--> Coasts
                                      +--> Ports
                                      +--> State
```

The CLI sends requests to `coastd` over a local Unix socket; Coastguard connects over a WebSocket. The daemon applies changes to runtime state.

## What It Does

`coastd` handles the operations that need persistent state and background coordination:

- Tracks Coast instances, builds, and shared services.
- Creates, starts, stops, and removes Coast runtimes.
- Applies assign/unassign/checkout operations.
- Manages canonical and dynamic [port forwarding](PORTS.md).
- Streams [logs](LOGS.md), status, and runtime events to CLI and UI clients.

In short: if you run `coast run`, `coast assign`, `coast checkout`, or `coast ls`, the daemon is the component doing the work.

## How It Runs

You can run the daemon in two common ways:

```bash
# Register daemon auto-start at login (recommended)
coast daemon install

# Manual start mode
coast daemon start
```

If you skip daemon install, you need to start it yourself each session before using Coast commands.

## Reporting Bugs

If you run into issues, please include the `coastd` daemon logs when submitting a bug report. The logs contain the context needed to diagnose most problems:

```bash
coast daemon logs
```

