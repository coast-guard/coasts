# Full-Stack Monorepo

This recipe is for a large monorepo with multiple web applications backed by a shared database and cache layer. The stack uses Docker Compose for the heavyweight backend services (Rails, Sidekiq, SSR) and runs Vite dev servers as bare services on the DinD host. Postgres and Redis run as shared services on the host Docker daemon so every Coast instance talks to the same infrastructure without duplicating it.

This pattern works well when:

- Your monorepo contains several apps that share a database
- You want lightweight Coast instances that do not each run their own Postgres and Redis
- Your frontend dev servers need to be reachable from inside compose containers via `host.docker.internal`
- You have host-side MCP integrations that connect to `localhost:5432` and want them to keep working unchanged

## The Complete Coastfile

Here is the full Coastfile. Each section is explained in detail below.

```toml
[coast]
name = "acme"
compose = "./infra/docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "bash"]
run = ["npm install -g yarn"]

[ports]
api = 3000
web = 3002
vite-web = 3040
vite-api = 3036

[healthcheck]
web = "/"
api = "/"

# --- Bare services: Vite dev servers on the DinD host ---

[services.vite-web]
install = "cd /workspace && yarn install --immutable 2>/dev/null || yarn install"
command = "cd /workspace && yarn workspace @acme/web run dev"
port = 3040
restart = "on-failure"
cache = ["node_modules"]

[services.vite-api]
command = "cd /workspace && yarn workspace @acme/api run dev"
port = 3036
restart = "on-failure"

# --- Shared services: Postgres and Redis on the host daemon ---

[shared_services.db]
image = "postgres:15.3-alpine"
ports = [5432]
volumes = ["infra_postgres:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "password" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis:/data"]

# --- Volumes: shared caches across all instances ---

[volumes.bundle]
strategy = "shared"
service = "api-rails"
mount = "/usr/local/bundle"

[volumes.api_rails_cache]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/tmp/cache"

[volumes.api_assets]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/public/assets"

[volumes.web_rails_cache]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/tmp/cache"

[volumes.web_assets]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/public/assets"

# --- Secrets and injection ---

[secrets.compose_profiles]
extractor = "command"
run = "echo api,web"
inject = "env:COMPOSE_PROFILES"

[secrets.uid]
extractor = "command"
run = "id -u"
inject = "env:UID"

[secrets.gid]
extractor = "command"
run = "id -g"
inject = "env:GID"

[inject]
env = ["USER", "BUNDLE_GEMS__CONTRIBSYS__COM"]

# --- Assign: branch-switch behavior ---

[assign]
default = "none"
exclude_paths = [
    ".coasts",
    ".yarn",
    ".github",
    "docs",
    "scripts",
    "cli",
    "deploy",
    "tools",
    "mobile",
    "extensions",
]

[assign.services]
web-rails = "hot"
web-ssr = "hot"
web-sidekiq = "restart"
api-rails = "hot"
api-sidekiq = "restart"

[assign.rebuild_triggers]
web-rails = ["infra/Dockerfile", "web/Gemfile", "web/Gemfile.lock", "web/package.json"]
api-rails = ["infra/Dockerfile", "api/Gemfile", "api/Gemfile.lock", "api/package.json"]
```

## Project and Compose

```toml
[coast]
name = "acme"
compose = "./infra/docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "bash"]
run = ["npm install -g yarn"]
```

The `compose` field points at your existing Docker Compose file. Coast runs `docker compose up -d` inside the DinD container on `coast run`, so your backend services (Rails servers, Sidekiq workers, SSR processes) start automatically.

`[coast.setup]` installs packages on the DinD host itself — not inside your compose containers. These are needed by the bare services (Vite dev servers) that run directly on the host. Your compose services get their runtimes from their Dockerfiles as usual.

## Shared Services

```toml
[shared_services.db]
image = "postgres:15.3-alpine"
ports = [5432]
volumes = ["infra_postgres:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "password" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis:/data"]
```

Postgres and Redis are declared as [shared services](../concepts_and_terminology/SHARED_SERVICES.md) rather than running inside each Coast. This means they run on the host Docker daemon, and every Coast instance connects to them over a bridge network.

**Why shared services instead of compose-internal databases?**

- **Lighter instances.** Each Coast skips spinning up its own Postgres and Redis containers, which saves memory and startup time.
- **Host volume reuse.** The `volumes` field references your existing Docker volumes (the ones created by your local `docker-compose up`). All the data you already have is immediately available — no seeding, no migration re-runs.
- **MCP compatibility.** If you have database MCP tools on your host connecting to `localhost:5432`, they keep working because the shared Postgres is on the host at that same port. No reconfiguration needed.

**The tradeoff:** there is no data isolation between Coast instances. Every instance reads and writes the same database. If your workflow needs per-instance databases, use [volume strategies](../concepts_and_terminology/VOLUMES.md) with `strategy = "isolated"` instead, or use `auto_create_db = true` on the shared service to get a per-instance database within the shared Postgres. See the [Shared Services Coastfile reference](../coastfiles/SHARED_SERVICES.md) for details.

**Volume naming matters.** The volume names (`infra_postgres`, `infra_redis`) must match the volumes that already exist on your host from running `docker-compose up` locally. If they do not match, the shared service will start with an empty volume. Run `docker volume ls` to check your existing volume names before writing this section.

## Bare Services

```toml
[services.vite-web]
install = "cd /workspace && yarn install --immutable 2>/dev/null || yarn install"
command = "cd /workspace && yarn workspace @acme/web run dev"
port = 3040
restart = "on-failure"
cache = ["node_modules"]

[services.vite-api]
command = "cd /workspace && yarn workspace @acme/api run dev"
port = 3036
restart = "on-failure"
```

Vite dev servers are defined as [bare services](../concepts_and_terminology/BARE_SERVICES.md) — plain processes running directly on the DinD host, outside of Docker Compose. This is the [mixed service types](../concepts_and_terminology/MIXED_SERVICE_TYPES.md) pattern.

**Why bare instead of compose?**

The primary reason is networking. Compose services that need to reach the Vite dev server (for SSR, asset proxying, or HMR WebSocket connections) can use `host.docker.internal` to reach bare services on the DinD host. This avoids complex Docker network configuration and matches how most monorepo setups configure `VITE_RUBY_HOST` or similar environment variables.

Bare services also get direct access to the bind-mounted `/workspace` filesystem without going through an inner container's overlay. This means Vite's file watcher responds faster to changes.

**`install` and `cache`:** The `install` field runs before the service starts and again on every `coast assign`. Here it runs `yarn install` to pick up dependency changes when switching branches. The `cache` field tells Coast to preserve `node_modules` across worktree switches so install runs are incremental rather than from scratch.

**Only one `install`:** Notice that `vite-api` has no `install` field. In a yarn workspaces monorepo, a single `yarn install` at the root installs dependencies for all workspaces. Putting it on only one service avoids running it twice.

## Ports and Healthchecks

```toml
[ports]
api = 3000
web = 3002
vite-web = 3040
vite-api = 3036

[healthcheck]
web = "/"
api = "/"
```

Every port you want Coast to manage goes in `[ports]`. Each instance gets a [dynamic port](../concepts_and_terminology/PORTS.md) (high range, always accessible) for each declared port. The [checked-out](../concepts_and_terminology/CHECKOUT.md) instance also gets the canonical port (the number you declared) forwarded to the host.

The `[healthcheck]` section tells Coast how to probe each port's health. For ports with a healthcheck path configured, Coast sends an HTTP GET every 5 seconds — any HTTP response counts as healthy. Ports without a healthcheck path fall back to a TCP connect check (can the port accept a connection?).

In this example, the Rails web servers get HTTP healthchecks at `/` because they serve HTML pages. The Vite dev servers are left without healthcheck paths — they do not serve a meaningful root page, and a TCP check is sufficient to know they are accepting connections.

Healthcheck status is visible in the [Coastguard](../concepts_and_terminology/COASTGUARD.md) UI and via `coast ports`.

## Volumes

```toml
[volumes.bundle]
strategy = "shared"
service = "api-rails"
mount = "/usr/local/bundle"

[volumes.api_rails_cache]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/tmp/cache"

[volumes.api_assets]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/public/assets"

[volumes.web_rails_cache]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/tmp/cache"

[volumes.web_assets]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/public/assets"
```

All volumes here use `strategy = "shared"`, which means a single Docker volume is shared across all Coast instances. This is the right choice for **caches and build artifacts** — things where concurrent writes are safe and duplicating per-instance would waste disk space and slow down startup:

- **`bundle`** — the Ruby gem cache. Gems are the same across branches. Sharing avoids re-downloading the entire bundle for each Coast instance.
- **`*_rails_cache`** — Rails file-based caches. These speed up development but are not precious — any instance can regenerate them.
- **`*_assets`** — compiled assets. Same reasoning as caches.

**Why not shared for databases?** Coast prints a warning if you use `strategy = "shared"` on a volume attached to a database-like service. Multiple Postgres processes writing to the same data directory causes corruption. For databases, either use [shared services](../coastfiles/SHARED_SERVICES.md) (one Postgres on the host, as this recipe does) or `strategy = "isolated"` (each Coast gets its own volume). See the [Volume Topology](../concepts_and_terminology/VOLUMES.md) page for the full decision matrix.

## Assign Strategies

```toml
[assign]
default = "none"
exclude_paths = [
    ".coasts",
    ".yarn",
    ".github",
    "docs",
    "scripts",
    "cli",
    "deploy",
    "tools",
    "mobile",
    "extensions",
]

[assign.services]
web-rails = "hot"
web-ssr = "hot"
web-sidekiq = "restart"
api-rails = "hot"
api-sidekiq = "restart"

[assign.rebuild_triggers]
web-rails = ["infra/Dockerfile", "web/Gemfile", "web/Gemfile.lock", "web/package.json"]
api-rails = ["infra/Dockerfile", "api/Gemfile", "api/Gemfile.lock", "api/package.json"]
```

The `[assign]` section controls what happens to each service when you run `coast assign` to switch a Coast instance to a different worktree. Getting this right is the difference between a 5-second branch switch and a 60-second one.

### `default = "none"`

Setting the default to `"none"` means any service not explicitly listed in `[assign.services]` is left untouched on branch switch. This is critical for databases and caches — Postgres, Redis, and infrastructure services do not change between branches and restarting them is wasted work.

### Per-service strategies

| Service | Strategy | Why |
|---|---|---|
| `web-rails`, `web-ssr`, `api-rails` | `hot` | These run dev servers with file watchers. The [filesystem remount](../concepts_and_terminology/FILESYSTEM.md) swaps the code under `/workspace` and the watcher picks up changes automatically. No container restart needed. |
| `web-sidekiq`, `api-sidekiq` | `restart` | Background workers load code at startup and do not watch for file changes. They need a container restart to pick up the new branch's code. |

Only list services that are actually running. If your `COMPOSE_PROFILES` only starts a subset of services, do not list inactive ones — Coast evaluates the assign strategy for every listed service, and restarting a service that is not running is wasted work. See [Performance Optimizations](../concepts_and_terminology/PERFORMANCE_OPTIMIZATIONS.md) for more on this.

### `exclude_paths`

This is the single most impactful optimization for large monorepos when new worktrees are being created. It tells Coast to skip entire directory trees while it bootstraps gitignored files into a worktree for the first time.

The goal is to exclude everything your Coast services do not need. In a monorepo with 30,000 files, the directories listed above might account for 8,000+ files that are irrelevant to the running services. Excluding them keeps the ignored-file bootstrap focused on the smaller subset your Coast actually needs.

To find what to exclude, profile your repo:

```bash
git ls-files --others --ignored --exclude-standard | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

Keep directories that contain source code mounted into running services, shared libraries imported by those services, or generated files the runtime needs on first boot. Exclude everything else — documentation, CI configs, tooling, other teams' apps, mobile clients, CLI tools, and vendored caches like `.yarn`.

### `rebuild_triggers`

Without triggers, a service with `strategy = "rebuild"` rebuilds its Docker image on every branch switch — even if nothing affecting the image changed. The `[assign.rebuild_triggers]` section gates the rebuild on specific files.

In this recipe, the Rails services normally use `"hot"` (no restart at all). But if someone changes the Dockerfile or Gemfile, the `rebuild_triggers` kick in and force a full image rebuild. If none of the trigger files changed, Coast skips the rebuild entirely. This avoids expensive image builds on routine code changes while still catching infrastructure-level changes.

## Secrets and Inject

```toml
[secrets.compose_profiles]
extractor = "command"
run = "echo api,web"
inject = "env:COMPOSE_PROFILES"

[secrets.uid]
extractor = "command"
run = "id -u"
inject = "env:UID"

[secrets.gid]
extractor = "command"
run = "id -g"
inject = "env:GID"

[inject]
env = ["USER", "BUNDLE_GEMS__CONTRIBSYS__COM"]
```

The `[secrets]` section extracts values at build time and injects them into Coast instances as environment variables.

- **`compose_profiles`** controls which Docker Compose profiles start. This is how you limit a Coast to running only the `api` and `web` profiles instead of every service defined in the compose file. Override it on your host with `export COMPOSE_PROFILES=api,web,portal` before building to change which services start.
- **`uid` / `gid`** pass the host user's UID and GID into the container, which is common for Docker setups that need file ownership to match between host and container.

The `[inject]` section is simpler — it forwards existing host environment variables into the Coast container at runtime. Sensitive credentials like gem server tokens (`BUNDLE_GEMS__CONTRIBSYS__COM`) stay on your host and are forwarded without being written to any config file.

For the full reference on secret extractors and injection targets, see [Secrets](../coastfiles/SECRETS.md).

## Adapting This Recipe

**Different language stack:** Replace the Rails-specific volumes (bundle, rails cache, assets) with equivalents for your stack — Go module cache (`/go/pkg/mod`), npm cache, pip cache, etc. The strategy stays `"shared"` for any cache that is safe to share across instances.

**Fewer apps:** If your monorepo has only one app, drop the extra volume entries and simplify `[assign.services]` to list only your services. The shared services and bare service patterns still apply.

**Per-instance databases:** If you need data isolation between Coast instances, replace `[shared_services.db]` with a compose-internal Postgres and add a `[volumes]` entry with `strategy = "isolated"`. Each instance gets its own database volume. You can seed it from your host volume using `snapshot_source` — see the [Volumes Coastfile reference](../coastfiles/VOLUMES.md).

**No bare services:** If your frontend is fully containerized and does not need to be reachable via `host.docker.internal`, remove the `[services.*]` sections and `[coast.setup]`. Everything runs through compose.
