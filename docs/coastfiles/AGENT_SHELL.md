# Agent Shell

> **In most workflows, you do not need to containerize your coding agent.** Because Coasts share the [filesystem](../concepts_and_terminology/FILESYSTEM.md) with your host machine, the simplest approach is to run the agent on your host and use [`coast exec`](../concepts_and_terminology/EXEC_AND_DOCKER.md) for runtime-heavy tasks like integration tests. Agent shells are for cases where you specifically want the agent running inside the container — for example, to give it direct access to the inner Docker daemon or to fully isolate its environment.

The `[agent_shell]` section configures an agent TUI — such as Claude Code or Codex — to run inside the Coast container. When present, Coast automatically spawns a persistent PTY session running the configured command when an instance starts.

For the full picture of how agent shells work — the active agent model, sending input, lifecycle and recovery — see [Agent Shells](../concepts_and_terminology/AGENT_SHELLS.md).

## Configuration

The section has a single required field: `command`.

```toml
[agent_shell]
command = "claude --dangerously-skip-permissions"
```

### `command` (required)

The shell command to run in the agent PTY. This is typically a coding agent CLI that you've installed via `[coast.setup]`.

The command runs inside the DinD container at `/workspace` (the project root). It is not a compose service — it runs alongside your compose stack or bare services, not inside them.

## Lifecycle

- The agent shell spawns automatically on `coast run`.
- In [Coastguard](../concepts_and_terminology/COASTGUARD.md), it appears as a persistent "Agent" tab that cannot be closed.
- If the agent process exits, Coast can respawn it.
- You can send input to a running agent shell via `coast agent-shell input`.

## Examples

### Claude Code

Install Claude Code in `[coast.setup]`, configure credentials via [secrets](SECRETS.md), then set up the agent shell:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "git", "bash"]
run = [
    "npm install -g @anthropic-ai/claude-code",
    "mkdir -p /root/.claude",
]

[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[agent_shell]
command = "cd /workspace; exec claude --dangerously-skip-permissions --effort high"
```

### Simple agent shell

A minimal agent shell for testing that the feature works:

```toml
[coast]
name = "test-agent"

[coast.setup]
packages = ["bash"]

[agent_shell]
command = "exec sh -c 'while true; do echo agent-heartbeat; sleep 5; done'"
```
