# Coastfiles

Coastfile — это конфигурационный файл TOML, который находится в корне вашего проекта. Он сообщает Coast всё, что нужно знать для сборки и запуска изолированных сред разработки для этого проекта — какие сервисы запускать, какие порты пробрасывать, как обрабатывать данные и как управлять секретами.

Каждому проекту Coast нужен как минимум один Coastfile. Файл всегда называется `Coastfile` (с заглавной C, без расширения). Если вам нужны варианты для разных рабочих процессов, вы создаёте типизированные Coastfiles, например `Coastfile.light` или `Coastfile.snap`, которые [наследуются от базового](INHERITANCE.md).

Чтобы глубже понять, как Coastfiles связаны с остальной частью Coast, см. [Coasts](../concepts_and_terminology/COASTS.md) и [Builds](../concepts_and_terminology/BUILDS.md).

## Quickstart

Наименьший возможный Coastfile:

```toml
[coast]
name = "my-app"
```

Это даёт вам контейнер DinD, в который можно войти через `coast exec`. Большинству проектов понадобится либо ссылка на `compose`, либо [bare services](SERVICES.md):

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
web = 3000
api = 8080
```

Или без compose, используя bare services:

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

Выполните `coast build`, затем `coast run dev-1`, и у вас будет изолированная среда.

## Example Coastfiles

### Simple bare-service project

Приложение Next.js без compose-файла. Coast устанавливает Node, выполняет `npm install` и напрямую запускает dev-сервер.

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

Проект с несколькими сервисами, с общими базами данных, секретами, стратегиями томов и пользовательской настройкой.

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

Расширяет базовый Coastfile, но упрощает его до того, что нужно только для запуска backend-тестов. Без портов, без общих сервисов, с изолированными базами данных.

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

Каждый экземпляр coast запускается с копией существующих томов базы данных хоста, а затем развивается независимо.

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

- Файл должен называться `Coastfile` (с заглавной C, без расширения) и находиться в корне проекта.
- Типизированные варианты используют шаблон `Coastfile.{type}` — например, `Coastfile.light`, `Coastfile.snap`. См. [Inheritance and Types](INHERITANCE.md).
- Зарезервированное имя `Coastfile.default` не допускается.
- Повсюду используется синтаксис TOML. Все заголовки секций используют `[brackets]`, а именованные записи используют `[section.name]` (не array-of-tables).
- Нельзя использовать одновременно `compose` и `[services]` в одном Coastfile — выберите что-то одно.
- Относительные пути (для `compose`, `root` и т. д.) разрешаются относительно родительского каталога Coastfile.

## Reference

| Page | Sections | What it covers |
|------|----------|----------------|
| [Project and Setup](PROJECT.md) | `[coast]`, `[coast.setup]` | Имя, путь к compose, runtime, каталог worktree, настройка контейнера |
| [Worktree Directories](WORKTREE_DIR.md) | `worktree_dir`, `default_worktree_dir` | Локальные и внешние каталоги worktree, пути с тильдой, интеграция с Codex/Claude |
| [Ports](PORTS.md) | `[ports]`, `[egress]` | Проброс портов, объявления egress, основной порт |
| [Volumes](VOLUMES.md) | `[volumes.*]` | Стратегии томов: изолированные, общие и инициализированные из snapshot |
| [Shared Services](SHARED_SERVICES.md) | `[shared_services.*]` | Базы данных и инфраструктурные сервисы на уровне хоста |
| [Secrets](SECRETS.md) | `[secrets.*]`, `[inject]` | Извлечение секретов, внедрение и проброс env/файлов хоста |
| [Bare Services](SERVICES.md) | `[services.*]` | Запуск процессов напрямую без Docker Compose |
| [Agent Shell](AGENT_SHELL.md) | `[agent_shell]` | Контейнеризированные TUI-рантаймы агента |
| [MCP Servers](MCP.md) | `[mcp.*]`, `[mcp_clients.*]` | Внутренние и проксируемые с хоста MCP-серверы, клиентские коннекторы |
| [Assign](ASSIGN.md) | `[assign]` | Поведение при переключении веток для каждого сервиса |
| [Inheritance and Types](INHERITANCE.md) | `extends`, `includes`, `[unset]`, `[omit]` | Типизированные Coastfiles, композиция и переопределения |
