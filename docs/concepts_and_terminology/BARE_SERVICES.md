# Bare Services

If you can containerize your project, you should. Bare services exist for projects that have not been containerized yet and where adding a `Dockerfile` and `docker-compose.yml` is not practical in the short term. They are a stepping stone, not a destination.

Instead of a `docker-compose.yml` orchestrating containerized services, bare services let you define shell commands in your Coastfile and Coast runs them as plain processes with a lightweight supervisor inside the Coast container.

## Why Containerize Instead

[Docker Compose](RUNTIMES_AND_SERVICES.md) services give you:

- Reproducible builds via Dockerfiles
- Health checks that Coast can wait on during startup
- Process isolation between services
- Volume and network management handled by Docker
- A portable definition that works in CI, staging, and production

Bare services give you none of that. Your processes share the same filesystem, crash recovery is a shell loop, and "works on my machine" is just as likely inside the Coast as outside it. If your project already has a `docker-compose.yml`, use it.

## When Bare Services Make Sense

- You are adopting Coast for a project that has never been containerized and you want to start getting value from worktree isolation and port management immediately
- Your project is a single-process tool or CLI where a Dockerfile would be overkill
- You want to iterate on containerizing gradually — start with bare services, move to compose later

## Configuration

Bare services are defined with `[services.<name>]` sections in your Coastfile. A Coastfile **cannot** define both `compose` and `[services]` — they are mutually exclusive.

```toml
[coast]
name = "my-app"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
web = 3000
```

Each service has four fields:

| Field | Required | Description |
|---|---|---|
| `command` | yes | The shell command to run (e.g. `"npm run dev"`) |
| `port` | no | The port the service listens on, used for port mapping |
| `restart` | no | Restart policy: `"no"` (default), `"on-failure"`, or `"always"` |
| `install` | no | One or more commands to run before starting (e.g. `"npm install"` or `["npm install", "npm run build"]`) |

### Setup Packages

Since bare services run as plain processes, the Coast container needs the right runtimes installed. Use `[coast.setup]` to declare system packages:

```toml
[coast.setup]
packages = ["nodejs", "npm"]
```

These are installed before any service starts. Without this, your `npm` or `node` commands will fail inside the container.

### Install Commands

The `install` field runs before the service starts and again on every [`coast assign`](ASSIGN.md) (branch switch). This is where dependency installation goes:

```toml
[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
```

Install commands run sequentially. If any install command fails, the service does not start.

### Restart Policies

- **`no`** — the service runs once. If it exits, it stays dead. Use this for one-shot tasks or services you want to manage manually.
- **`on-failure`** — restarts the service if it exits with a non-zero code. Successful exits (code 0) are left alone. Uses exponential backoff from 1 second up to 30 seconds, and gives up after 10 consecutive crashes.
- **`always`** — restarts on any exit, including success. Same backoff as `on-failure`. Use this for long-running servers that should never stop.

If a service runs for more than 30 seconds before crashing, the retry counter and backoff reset — the assumption is that it was healthy for a while and the crash is a new problem.

## How It Works Under the Hood

```text
┌─── Coast: dev-1 ──────────────────────────────────────┐
│                                                       │
│   /coast-supervisor/                                  │
│   ├── web.sh          (runs command, tracks PID)      │
│   ├── worker.sh                                       │
│   ├── start-all.sh    (launches all services)         │
│   ├── stop-all.sh     (SIGTERM via PID files)         │
│   └── ps.sh           (checks PID liveness)           │
│                                                       │
│   /var/log/coast-services/                            │
│   ├── web.log                                         │
│   └── worker.log                                      │
│                                                       │
│   No inner Docker daemon images are used.             │
│   Processes run directly on the container OS.         │
└───────────────────────────────────────────────────────┘
```

Coast generates shell-script wrappers for each service and places them in `/coast-supervisor/` inside the DinD container. Each wrapper tracks its PID, redirects output to a log file, and implements the restart policy as a shell loop. There is no Docker Compose, no inner Docker images, and no container-level isolation between services.

`coast ps` checks PID liveness rather than querying Docker, and `coast logs` tails the log files rather than calling `docker compose logs`. The log output format matches compose's `service | line` format so Coastguard's UI works without changes.

## Ports

Port configuration works exactly the same as with compose-based Coasts. Define the ports your services listen on in `[ports]`:

```toml
[services.web]
command = "npm start"
port = 3000

[ports]
web = 3000
```

[Dynamic ports](PORTS.md) are allocated on `coast run`, and [`coast checkout`](CHECKOUT.md) swaps canonical ports as usual. The only difference is that there is no Docker network between services — they all bind directly to the container's loopback or `0.0.0.0`.

## Branch Switching

When you run `coast assign` on a bare-services Coast, the following happens:

1. All running services are stopped via SIGTERM
2. The worktree switches to the new branch
3. Install commands re-run (e.g. `npm install` picks up the new branch's dependencies)
4. All services restart

This is equivalent to what happens with compose — `docker compose down`, branch switch, rebuild, `docker compose up` — but with shell processes instead of containers.

## Limitations

- **No health checks.** Coast cannot wait for a bare service to be "healthy" the way it can with a compose service that defines a health check. It starts the process and hopes for the best.
- **No isolation between services.** All processes share the same filesystem and process namespace inside the Coast container. A misbehaving service can affect others.
- **No build caching.** Docker Compose builds are cached layer by layer. Bare service `install` commands run from scratch on every assign.
- **Crash recovery is basic.** The restart policy uses a shell loop with exponential backoff. It is not a process supervisor like systemd or supervisord.
- **No `[omit]` or `[unset]` for services.** Coastfile type composition works with compose services, but bare services do not support omitting individual services via typed Coastfiles.

## Migrating to Compose

When you are ready to containerize, the migration path is straightforward:

1. Write a `Dockerfile` for each service
2. Create a `docker-compose.yml` that references them
3. Replace the `[services.*]` sections in your Coastfile with a `compose` field pointing to your compose file
4. Remove `[coast.setup]` packages that are now handled by your Dockerfiles
5. Rebuild with [`coast build`](BUILDS.md)

Your port mappings, [volumes](VOLUMES.md), [shared services](SHARED_SERVICES.md), and [secrets](SECRETS.md) configuration all carry over unchanged. The only thing that changes is how the services themselves run.
