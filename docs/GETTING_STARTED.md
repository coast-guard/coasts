# Getting Started with Coasts

```youtube
Je921fgJ4RY
Part of the [Coasts Video Course](learn-coasts-videos/README.md).
```

## Installing

```bash
curl -fsSL https://coasts.dev/install | sh
coast daemon install
```

*If you decide not to run `coast daemon install`, you are responsible for starting the daemon manually with `coast daemon start` every single time.*

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

## Setting Up Coasts in a Project

Add a Coastfile to the root of your project. Make sure you are not on a worktree when installing.

```text
my-project/
├── Coastfile              <-- this is what Coast reads
├── docker-compose.yml
├── Dockerfile
├── src/
│   └── ...
└── ...
```

The `Coastfile` points at your existing local development resources and adds Coasts-specific configuration — see the [Coastfiles documentation](coastfiles/README.md) for the full schema:

```toml
[coast]
name = "my-project"
compose = "./docker-compose.yml"

[ports]
web = 3000
db = 5432
```

A Coastfile is a lightweight TOML file that *typically* points to your existing `docker-compose.yml` (it also works with non-containerized local dev setups) and describes the modifications needed to run your project in parallel — port mappings, volume strategies, and secrets. Place it at your project root.

The fastest way to create a Coastfile for your project is to let your coding agent do it.

The Coasts CLI ships with a built-in prompt that teaches any AI agent the full Coastfile schema and CLI. Copy it into your agent's chat and it will analyze your project and generate a Coastfile.

```prompt-copy
installation_prompt.txt
```

You can also get the same output from the CLI by running `coast installation-prompt`.

## Your First Coast

Before starting your first Coast, bring down any running development environment. If you are using Docker Compose, run `docker-compose down`. If you have local dev servers running, stop them. Coasts manage their own ports and will conflict with anything already listening.

Once your Coastfile is ready:

```bash
coast build
coast run dev-1
```

Check that your instance is running:

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -             ~/dev/my-project
```

See where your services are listening:

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

Each instance gets its own set of dynamic ports so multiple instances can run side by side. To map an instance back to your project's canonical ports, check it out:

```bash
coast checkout dev-1
```

This means the runtime is now checked out and your project's canonical ports (like `3000`, `5432`) will route to this Coast instance.

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -         ✓   ~/dev/my-project
```

To bring up the Coastguard observability UI for your project:

```bash
coast ui
```

## What's Next?

- Set up a [skill for your host agent](SKILLS_FOR_HOST_AGENTS.md) so it knows how to interact with Coasts
