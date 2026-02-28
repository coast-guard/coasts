# Bare Services

> **Note:** Bare services run directly inside the Coast container as plain processes — they are not containerized. If your services are already Dockerized, use `compose` instead. Bare services are best suited for simple setups where you want to skip the overhead of writing a Dockerfile and docker-compose.yml.

The `[services.*]` sections define processes that Coast runs directly inside the DinD container, without Docker Compose. This is an alternative to using a `compose` file — you cannot use both in the same Coastfile.

Bare services are supervised by Coast with log capture and optional restart policies. For deeper background on how bare services work, their limitations, and when to migrate to compose, see [Bare Services](../concepts_and_terminology/BARE_SERVICES.md).

## Defining a service

Each service is a named TOML section under `[services]`. The `command` field is required.

```toml
[services.web]
command = "node server.js"
port = 3000
```

### `command` (required)

The shell command to run. Must not be empty or whitespace-only.

```toml
[services.web]
command = "npx next dev --turbopack --port 3000 --hostname 0.0.0.0"
```

### `port`

The port the service listens on. Used for health checking and port forwarding integration. Must be non-zero if specified.

```toml
[services.web]
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

### `restart`

Restart policy if the process exits. Defaults to `"no"`.

- `"no"` — do not restart
- `"on-failure"` — restart only if the process exits with a non-zero code
- `"always"` — always restart

```toml
[services.web]
command = "node server.js"
port = 3000
restart = "on-failure"
```

### `install`

Commands to run before starting the service (e.g. installing dependencies). Accepts either a single string or an array of strings.

```toml
[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

```toml
[services.web]
install = ["npm install", "npm run build"]
command = "npm start"
port = 3000
```

## Mutual exclusion with compose

A Coastfile cannot define both `compose` and `[services]`. If you have a `compose` field in `[coast]`, adding any `[services.*]` section is an error. Choose one approach per Coastfile.

If you need some services containerized via compose and some running bare, use compose for all of them — see [the migration guidance in Bare Services](../concepts_and_terminology/BARE_SERVICES.md) for how to move from bare services to compose.

## Examples

### Single-service Next.js app

```toml
[coast]
name = "my-frontend"

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

### Web server with background worker

```toml
[coast]
name = "my-app"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "node server.js"
port = 3000
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
web = 3000
```

### Python service with multi-step install

```toml
[coast]
name = "ml-service"

[coast.setup]
packages = ["python3", "py3-pip"]

[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
restart = "on-failure"

[ports]
api = 8000
```
