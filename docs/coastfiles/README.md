# Coastfiles

A Coastfile is a TOML configuration file that lives at the root of your project. It tells Coast everything it needs to know to build and run isolated development environments for that project — which services to run, which ports to forward, how to handle data, and how to manage secrets.

Every Coast project needs at least one Coastfile. The file is always named `Coastfile` (capital C, no extension). If you need variants for different workflows, you create typed Coastfiles like `Coastfile.light` or `Coastfile.snap` that [inherit from the base](INHERITANCE.md).

For a deeper understanding of how Coastfiles relate to the rest of Coast, see [Coasts](../concepts_and_terminology/COASTS.md) and [Builds](../concepts_and_terminology/BUILDS.md).

## Quickstart

The smallest possible Coastfile:

```toml
[coast]
name = "my-app"
```

This gives you a DinD container you can `coast exec` into. Most projects will want either a `compose` reference or [bare services](SERVICES.md):

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
web = 3000
api = 8080
```

Or without compose, using bare services:

```toml
[coast]
name = "my-app"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
restart = "on-failure"

[ports]
web = 3000
```

Run `coast build` then `coast run dev-1` and you have an isolated environment.

## Example Coastfiles

### Simple bare-service project

A Next.js app with no compose file. Coast installs Node, runs `npm install`, and starts the dev server directly.

```toml
[coast]
name = "my-crm"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --turbopack --port 3002 --hostname 0.0.0.0"
port = 3002
restart = "on-failure"

[ports]
web = 3002
```

### Full-stack compose project

A multi-service project with shared databases, secrets, volume strategies, and custom setup.

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
primary_port = "web"

[coast.setup]
packages = ["nodejs", "npm", "python3", "curl", "git", "bash", "ca-certificates", "wget"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]

[ports]
web = 3000
backend = 8080
postgres = 5432
redis = 6379

[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass" }

[shared_services.redis]
image = "redis:7"
ports = [6379]

[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"

[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"

[omit]
services = ["monitoring", "admin-panel", "nginx-proxy"]

[assign]
default = "none"
[assign.services]
backend = "hot"
web = "hot"
```

### Lightweight test variant (inheritance)

Extends the base Coastfile but strips it down to only what's needed for running backend tests. No ports, no shared services, isolated databases.

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "backend", "postgres", "redis"]
shared_services = ["postgres", "redis"]

[omit]
services = ["redis", "backend", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[assign]
default = "none"
[assign.services]
backend-test = "rebuild"
```

### Snapshot-seeded variant

Each coast instance starts with a copy of the host's existing database volumes, then diverges independently.

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis", "mongodb"]

[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "infra_redis_data"
service = "redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
snapshot_source = "infra_mongodb_data"
service = "mongodb"
mount = "/data/db"
```

## Conventions

- The file must be named `Coastfile` (capital C, no extension) and live at the project root.
- Typed variants use the pattern `Coastfile.{type}` — for example `Coastfile.light`, `Coastfile.snap`. See [Inheritance and Types](INHERITANCE.md).
- The reserved name `Coastfile.default` is not allowed.
- TOML syntax is used throughout. All section headers use `[brackets]` and named entries use `[section.name]` (not array-of-tables).
- You cannot use both `compose` and `[services]` in the same Coastfile — pick one.
- Relative paths (for `compose`, `root`, etc.) are resolved against the Coastfile's parent directory.

## Reference

| Page | Sections | What it covers |
|------|----------|----------------|
| [Project and Setup](PROJECT.md) | `[coast]`, `[coast.setup]` | Name, compose path, runtime, worktree dir, container setup |
| [Worktree Directories](WORKTREE_DIR.md) | `worktree_dir`, `default_worktree_dir` | Local and external worktree dirs, tilde paths, Codex/Claude integration |
| [Ports](PORTS.md) | `[ports]`, `[egress]` | Port forwarding, egress declarations, primary port |
| [Volumes](VOLUMES.md) | `[volumes.*]` | Isolated, shared, and snapshot-seeded volume strategies |
| [Shared Services](SHARED_SERVICES.md) | `[shared_services.*]` | Host-level databases and infrastructure services |
| [Secrets](SECRETS.md) | `[secrets.*]`, `[inject]` | Secret extraction, injection, and host env/file forwarding |
| [Bare Services](SERVICES.md) | `[services.*]` | Running processes directly without Docker Compose |
| [Agent Shell](AGENT_SHELL.md) | `[agent_shell]` | Containerized agent TUI runtimes |
| [MCP Servers](MCP.md) | `[mcp.*]`, `[mcp_clients.*]` | Internal and host-proxied MCP servers, client connectors |
| [Assign](ASSIGN.md) | `[assign]` | Branch-switch behavior per service |
| [Inheritance and Types](INHERITANCE.md) | `extends`, `includes`, `[unset]`, `[omit]` | Typed Coastfiles, composition, and overrides |
