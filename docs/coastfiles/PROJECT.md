# Project and Setup

The `[coast]` section is the only required section in a Coastfile. It identifies the project and configures how the Coast container is created. The optional `[coast.setup]` subsection lets you install packages and run commands inside the container at build time.

## `[coast]`

### `name` (required)

A unique identifier for the project. Used in container names, volume names, state tracking, and CLI output.

```toml
[coast]
name = "my-app"
```

### `compose`

Path to a Docker Compose file. Relative paths are resolved against the project root (the directory containing the Coastfile, or `root` if set).

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
```

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
```

If omitted, the Coast container starts without running `docker compose up`. You can either use [bare services](SERVICES.md) or interact with the container directly via `coast exec`.

You cannot set both `compose` and `[services]` in the same Coastfile.

### `runtime`

Which container runtime to use. Defaults to `"dind"` (Docker-in-Docker).

- `"dind"` — Docker-in-Docker with `--privileged`. The only production-tested runtime. See [Runtimes and Services](../concepts_and_terminology/RUNTIMES_AND_SERVICES.md).
- `"sysbox"` — Uses the Sysbox runtime instead of privileged mode. Requires Sysbox to be installed.
- `"podman"` — Uses Podman as the inner container runtime.

```toml
[coast]
name = "my-app"
runtime = "dind"
```

### `root`

Overrides the project root directory. By default, the project root is the directory containing the Coastfile. A relative path is resolved against the Coastfile's directory; an absolute path is used as-is.

```toml
[coast]
name = "my-app"
root = "../my-project"
```

This is uncommon. Most projects keep the Coastfile at the actual project root.

### `worktree_dir`

Directories where git worktrees live. Accepts a single string or an array of strings. Defaults to `".worktrees"`.

```toml
# Single directory
worktree_dir = ".worktrees"

# Multiple directories, including an external one
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

Relative paths are resolved against the project root. Paths starting with `~/` or `/` are treated as **external** directories — Coast adds a separate bind mount so the container can access them. This is how you integrate with tools like Codex that create worktrees outside the project root.

At runtime, Coast auto-detects the worktree directory from existing git worktrees (via `git worktree list`) and prefers that over the configured default when all worktrees agree on a single directory.

See [Worktree Directories](WORKTREE_DIR.md) for the full reference, including external directory behavior, project filtering, and examples.

### `default_worktree_dir`

Which directory to use when creating **new** worktrees. Defaults to the first entry in `worktree_dir`. Only relevant when `worktree_dir` is an array.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
default_worktree_dir = ".worktrees"
```

### `autostart`

Whether to automatically run `docker compose up` (or start bare services) when a Coast instance is created with `coast run`. Defaults to `true`.

Set to `false` when you want the container running but want to start services manually — useful for test-runner variants where you invoke tests on demand.

```toml
[coast]
name = "my-app"
extends = "Coastfile"
autostart = false
```

### `primary_port`

Names a port from the `[ports]` section to use for quick-links and subdomain routing. The value must match a key defined in `[ports]`.

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

See [Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md) for how this enables subdomain routing and URL templates.

## `[coast.setup]`

Customizes the Coast container itself — installing tools, running build steps, and materializing config files. Everything in `[coast.setup]` runs inside the DinD container (not inside your compose services).

### `packages`

APK packages to install. These are Alpine Linux packages since the base DinD image is Alpine-based.

```toml
[coast.setup]
packages = ["nodejs", "npm", "git", "curl"]
```

### `run`

Shell commands executed in order during build. Use these for installing tools that aren't available as APK packages.

```toml
[coast.setup]
packages = ["nodejs", "npm", "python3", "wget", "bash", "ca-certificates"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]
```

### `[[coast.setup.files]]`

Files to create inside the container. Each entry has a `path` (required, must be absolute), `content` (required), and optional `mode` (3-4 digit octal string).

```toml
[coast.setup]
packages = ["nodejs", "npm"]
run = ["mkdir -p /app/config"]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```

Validation rules for file entries:

- `path` must be absolute (start with `/`)
- `path` must not contain `..` components
- `path` must not end with `/`
- `mode` must be a 3 or 4 digit octal string (e.g. `"600"`, `"0644"`)

## Full example

A Coast container set up for Go and Node.js development:

```toml
[coast]
name = "my-fullstack-app"
compose = "./docker-compose.yml"
runtime = "dind"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
primary_port = "web"

[coast.setup]
packages = ["nodejs", "npm", "python3", "make", "curl", "git", "bash", "ca-certificates", "wget", "gcc", "musl-dev"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz && ln -s /usr/local/go/bin/go /usr/local/bin/go",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
    "pip3 install --break-system-packages pgcli",
]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```
