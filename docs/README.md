# Coasts Documentation

```youtube
MBGKSKau4sU
Part of the [Coasts Video Course](learn-coasts-videos/README.md).
```

## Installing

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*If you decide not to run `coast daemon install`, you are responsible for starting the daemon manually with `coast daemon start` every single time.*

## What Are Coasts?

A Coast (**containerized host**) is a local development runtime. Coasts let you run multiple isolated environments for the same project on one machine.

Coasts are especially useful for complex `docker-compose` stacks with many interdependent services, but they are equally effective for non-containerized local dev setups. Coasts support a wide range of [runtime configuration patterns](concepts_and_terminology/RUNTIMES_AND_SERVICES.md) so you can shape the ideal environment for multiple agents working in parallel.

Coasts are built for local development, not as a hosted cloud service. Your environments run locally on your machine.

The Coasts project is free, local, MIT-licensed, agent-provider agnostic, and agent-harness agnostic software with no AI upsells.

Coasts work with any agentic coding workflow that uses worktrees. No special harness-side configuration is required.

## Why Coasts for Worktrees

Git worktrees are excellent for isolating code changes, but they do not solve runtime isolation by themselves.

When you run multiple worktrees in parallel, you quickly hit ergonomic problems:

- [Port conflicts](concepts_and_terminology/PORTS.md) between services that expect the same host ports.
- Per-worktree database and [volume setup](concepts_and_terminology/VOLUMES.md) that is tedious to manage.
- Integration test environments that need custom runtime wiring per worktree.
- The living hell of switching worktrees and rebuilding runtime context each time. See [Assign and Unassign](concepts_and_terminology/ASSIGN.md).

If Git is version control for your code, Coasts are like Git for your worktree runtimes.

Each environment gets its own ports, so you can inspect any worktree runtime in parallel. When you [check out](concepts_and_terminology/CHECKOUT.md) a worktree runtime, Coasts remap that runtime to your project's canonical ports.

Coasts abstract runtime configuration into a simple modular layer on top of worktrees, so each worktree can run with the isolation it needs without hand-maintaining complex per-worktree setup.

## Requirements

- macOS or Linux
- Docker Desktop on macOS, or Docker Engine with the Compose plugin on Linux
- A project using Git
- Node.js
- `socat` (`brew install socat` on macOS, `sudo apt install socat` on Ubuntu)

```text
Linux note: Dynamic ports work out of the box on Linux.
If you need canonical ports below `1024`, see the checkout docs for the required host configuration.
```

## Containerizing Agents?

You can containerize an agent with a Coast. That might sound like a great idea at first, but in many cases you do not actually need to run your coding agent inside a container.

Because Coasts share the [filesystem](concepts_and_terminology/FILESYSTEM.md) with your host machine through a shared volume mount, the easiest and most reliable workflow is to run the agent on your host and instruct it to execute runtime-heavy tasks (such as integration tests) inside the Coast instance using [`coast exec`](concepts_and_terminology/EXEC_AND_DOCKER.md).

However, if you do want to run your agent in a container, Coasts absolutely support that via [Agent Shells](concepts_and_terminology/AGENT_SHELLS.md). You can build an incredibly intricate rig for this setup including [MCP server configuration](concepts_and_terminology/MCP_SERVERS.md), but it may not interoperate cleanly with the orchestration software that exists today. For most workflows, host-side agents are simpler and more reliable.

## Coasts vs Dev Containers

Coasts are not dev containers, and they are not the same thing.

Dev containers are generally designed for mounting an IDE into a single containerized development workspace. Coasts are headless and optimized as lightweight environments for parallel agent usage with worktrees — multiple isolated, worktree-aware runtime environments running side by side, with fast checkout switching and runtime isolation controls for each instance.

## Demo Repo

If you want a small example project to try with Coasts, start with the [`coasts-demo` repository](https://github.com/coast-guard/coasts-demo).

## Coasts Video Course

If you prefer video, the [Coasts Video Course](learn-coasts-videos/README.md) covers every core concept in under three minutes each.

