# Lookup

`coast lookup` discovers which Coast instances are running for the caller's current working directory. It is the first command a host-side agent should run to orient itself — "I'm editing code here, which Coast(s) should I interact with?"

```bash
coast lookup
```

Lookup detects whether you are inside a [worktree](ASSIGN.md) or at the project root, queries the daemon for matching instances, and prints the results with ports, URLs, and example commands.

## Why This Exists

An AI coding agent running on the host (Cursor, Claude Code, Codex, etc.) edits files through the [shared filesystem](FILESYSTEM.md) and calls Coast CLI commands for runtime operations. But the agent first needs to answer a basic question: **which Coast instance corresponds to the directory I'm working in?**

Without `coast lookup`, the agent would have to run `coast ls`, parse the full instance table, figure out which worktree it's in, and cross-reference. `coast lookup` does all of that in one step and returns structured output that agents can consume directly.

This command should be included in any top-level SKILL.md, AGENTS.md, or rules file for agent workflows that use Coast. It is the entry point for an agent to discover its runtime context.

## Output Modes

### Default (human-readable)

```bash
coast lookup
```

```text
Coast instances for worktree feature/oauth (my-app):

  dev-1  running  ★ checked out

  Primary URL:  http://dev-1.localhost:62217

  SERVICE              CANONICAL       DYNAMIC
  ★ web                3000            62217
    api                8080            63889
    postgres           5432            55681

  Examples (exec starts at the workspace root where your Coastfile is, cd to your target directory first):
    coast exec dev-1 -- sh -c "cd <dir> && <command>"
    coast logs dev-1 --service <service>
    coast ps dev-1
```

The examples section reminds agents (and humans) that `coast exec` starts at the workspace root — the directory where the Coastfile lives. To run a command in a subdirectory, `cd` to it inside the exec.

### Compact (`--compact`)

Returns a JSON array of instance names. Designed for scripts and agent tooling that just needs to know which instances to target.

```bash
coast lookup --compact
```

```text
["dev-1"]
```

Multiple instances on the same worktree:

```text
["dev-1","dev-2"]
```

No matches:

```text
[]
```

### JSON (`--json`)

Returns the full structured response as pretty-printed JSON. Designed for agents that need ports, URLs, and status in a machine-readable format.

```bash
coast lookup --json
```

```json
{
  "project": "my-app",
  "worktree": "feature/oauth",
  "project_root": "/Users/dev/my-app",
  "instances": [
    {
      "name": "dev-1",
      "status": "Running",
      "checked_out": true,
      "branch": "feature/oauth",
      "primary_url": "http://dev-1.localhost:62217",
      "ports": [
        { "logical_name": "web", "canonical_port": 3000, "dynamic_port": 62217, "is_primary": true },
        { "logical_name": "api", "canonical_port": 8080, "dynamic_port": 63889, "is_primary": false }
      ]
    }
  ]
}
```

## How It Resolves

Lookup walks up from the current working directory to find the nearest Coastfile, then determines which worktree you are in:

1. If your cwd is under `{project_root}/{worktree_dir}/{name}/...`, lookup finds instances assigned to that worktree.
2. If your cwd is the project root (or any directory not inside a worktree), lookup finds instances with **no worktree assigned** — those still pointed at the project root.

This means lookup works from subdirectories too. If you are in `my-app/.coasts/feature-oauth/src/api/`, lookup still resolves `feature-oauth` as the worktree.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | One or more matching instances found |
| 1 | No matching instances (empty result) |

This makes lookup usable in shell conditionals:

```bash
if coast lookup > /dev/null 2>&1; then
  coast exec dev-1 -- sh -c "cd src && npm test"
fi
```

## For Agent Workflows

The typical agent integration pattern:

1. Agent starts working in a worktree directory.
2. Agent runs `coast lookup` to discover instance names, ports, URLs, and example commands.
3. Agent uses the instance name for all subsequent Coast commands: `coast exec`, `coast logs`, `coast ps`.

```text
┌─── Agent (host machine) ────────────────────────────┐
│                                                      │
│  1. coast lookup                                     │
│       → instance names, ports, URLs, examples        │
│  2. coast exec dev-1 -- sh -c "cd src && npm test"   │
│  3. coast logs dev-1 --service web --tail 50         │
│  4. coast ps dev-1                                   │
│                                                      │
└──────────────────────────────────────────────────────┘
```

If the agent is working across multiple worktrees, it runs `coast lookup` from each worktree directory to resolve the correct instance for each context.

See also [Filesystem](FILESYSTEM.md) for how host agents interact with Coast, [Assign and Unassign](ASSIGN.md) for worktree concepts, and [Exec & Docker](EXEC_AND_DOCKER.md) for running commands inside a Coast.
