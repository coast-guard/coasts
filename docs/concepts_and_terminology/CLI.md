# Coast CLI

The Coast CLI (`coast`) is the primary command-line interface for operating Coasts. It is intentionally thin: it parses your command, sends a request to [`coastd`](DAEMON.md), and prints structured output back to your terminal.

## What You Use It For

Typical workflows are all driven from the CLI:

```bash
coast build                                    # see Builds
coast run dev-1                                # see Run
coast assign dev-1 --worktree feature/oauth    # see Assign
coast ports dev-1                              # see Ports
coast checkout dev-1                           # see Checkout
coast ui                                       # see Coastguard
```

The CLI also includes documentation commands that are useful for humans and agents:

```bash
coast docs
coast docs --path concepts_and_terminology/CHECKOUT.md
coast search-docs "canonical vs dynamic ports"
```

## Why It Exists Separately from the Daemon

Separating CLI from daemon gives you a few important benefits:

- The daemon keeps state and long-lived processes.
- The CLI stays fast, composable, and easy to script.
- You can run one-off commands without keeping terminal state alive.
- Agent tooling can call CLI commands in predictable, automation-friendly ways.

## CLI vs Coastguard

Use whichever interface fits the moment:

- The CLI is designed for full operational coverage: anything you can do in Coastguard should also be possible from the CLI.
- Treat the CLI as the automation interface — scripts, agent workflows, CI jobs, and custom developer tooling.
- Treat [Coastguard](COASTGUARD.md) as the human interface — visual inspection, interactive debugging, and operational visibility.

Both talk to the same daemon, so they operate on the same underlying project state.
