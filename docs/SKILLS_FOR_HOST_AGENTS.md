# Skills for Host Agents

If you are using AI coding agents (Claude Code, Codex, Conductor, Cursor, or similar) on a project that uses Coasts, your agent needs a skill that teaches it how to interact with the Coast runtime. Without this, the agent will edit files but won't know how to run tests, check logs, or verify that its changes work inside the running environment.

This guide walks through setting up that skill.

## Why Agents Need This

Coasts share the [filesystem](concepts_and_terminology/FILESYSTEM.md) between your host machine and the Coast container. Your agent edits files on the host and the running services inside the Coast see the changes immediately. But the agent still needs to:

1. **Discover which Coast instance it's working with** — `coast lookup` resolves this from the agent's current directory.
2. **Run commands inside the Coast** — tests, builds, and other runtime tasks happen inside the container via `coast exec`.
3. **Read logs and check service status** — `coast logs` and `coast ps` give the agent runtime feedback.

The skill below teaches the agent all three.

## The Skill

Add the following to your agent's existing skill, rules, or prompt file. If your agent already has instructions for running tests or interacting with your dev environment, this belongs alongside those — it teaches the agent how to use Coasts for runtime operations.

```text-copy
This project uses Coasts (containerized host) for isolated development environments.
Your code edits are automatically visible inside the running Coast — the filesystem
is shared between the host and the container.

=== ORIENTATION ===

Before running any runtime commands, discover which Coast instance matches your
current working directory:

  coast lookup

This prints the instance name, ports, URLs, and example commands. Use the instance
name from the output for all subsequent commands.

If you need deeper context on how Coasts work, read these docs:

  coast docs --path concepts_and_terminology/LOOKUP.md
  coast docs --path concepts_and_terminology/FILESYSTEM.md
  coast docs --path concepts_and_terminology/EXEC_AND_DOCKER.md
  coast docs --path concepts_and_terminology/LOGS.md

=== RUNNING COMMANDS ===

Use `coast exec` to run commands inside the Coast. The shell starts at the workspace
root (where the Coastfile is). cd to your target directory first:

  coast exec <instance> -- sh -c "cd <dir> && <command>"

Examples:

  coast exec dev-1 -- sh -c "cd src && npm test"
  coast exec dev-1 -- sh -c "cd backend && go test ./..."
  coast exec dev-1 -- sh -c "cd apps/web && npx playwright test"

=== RUNTIME FEEDBACK ===

Check service status:

  coast ps <instance>

Read service logs:

  coast logs <instance> --service <service>
  coast logs <instance> --service <service> --tail 50

=== TROUBLESHOOTING ===

If you encounter errors or unfamiliar behavior, search the Coast docs:

  coast search-docs "error message or description"

This uses semantic search — describe the problem in natural language and it will
find the relevant documentation.

=== RULES ===

- Always run `coast lookup` before your first runtime command in a session.
- Do not run services directly on the host. Use `coast exec` for all runtime tasks.
- File edits on the host are instantly visible inside the Coast. You do not need
  to copy files or rebuild after editing.
- If `coast lookup` returns no instances, the Coast may not be running. Suggest
  `coast run dev-1` or check `coast ls` for the project state.
```

## Adding the Skill to Your Agent

How you add this depends on your agent:

### Claude Code

Add the skill text to your project's `CLAUDE.md` file, or create a dedicated section for it.

### Codex

Add the skill text to your project's `AGENTS.md` file.

### Cursor

Create a rules file at `.cursor/rules/coast.mdc` (or `.cursor/rules/coast.md`) in your project root and paste the skill text above.

### Other agents

Most agents support some form of project-level prompt or rules file. Paste the skill text into whatever your agent reads at session start.

## Further Reading

- Read the [Coastfiles documentation](coastfiles/README.md) to learn the full configuration schema
- Learn the [Coast CLI](concepts_and_terminology/CLI.md) commands for managing instances
- Explore [Coastguard](concepts_and_terminology/COASTGUARD.md), the web UI for observing and controlling your Coasts
- Browse [Concepts & Terminology](concepts_and_terminology/README.md) for the full picture of how Coasts work
