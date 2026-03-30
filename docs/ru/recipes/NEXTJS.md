# Приложение Next.js

Этот рецепт предназначен для приложения Next.js, работающего с Postgres и Redis, с необязательными фоновыми воркерами или сопутствующими сервисами. Стек запускает Next.js как [bare service](../concepts_and_terminology/BARE_SERVICES.md) с Turbopack для быстрого HMR, в то время как Postgres и Redis запускаются как [shared services](../concepts_and_terminology/SHARED_SERVICES.md) на хосте, чтобы каждый экземпляр Coast использовал одни и те же данные.

Этот шаблон хорошо подходит, когда:

- Ваш проект использует Next.js с Turbopack в разработке
- У вас есть слой базы данных и кэша (Postgres, Redis), поддерживающий приложение
- Вы хотите запускать несколько экземпляров Coast параллельно без настройки базы данных для каждого экземпляра
- Вы используете библиотеки аутентификации, такие как NextAuth, которые встраивают callback URL в ответы

## Полный Coastfile

```toml
[coast]
name = "my-nextjs-app"
primary_port = "web"
private_paths = ["packages/web/.next"]
worktree_dir = [".worktrees", ".claude/worktrees"]

[coast.setup]
packages = ["nodejs", "npm", "make", "git", "bash"]
run = [
    "npm install -g corepack",
    "corepack enable",
]

# --- Bare services: Next.js and background worker ---

[services.web]
install = [
    "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)",
    "cd /workspace && test -f config.json || echo {} > config.json",
    "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn prisma migrate dev",
]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev"
port = 3000
restart = "on-failure"
cache = ["node_modules"]

[services.worker]
install = "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)"
command = "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev:worker"
restart = "on-failure"
cache = ["node_modules"]

# --- Shared services: Postgres and Redis on the host ---

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["myapp_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_DB = "postgres", POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "postgres" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["myapp_redis_data:/data"]

# --- Secrets: connection strings for bare services ---

[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"

[secrets.redis_url]
extractor = "command"
run = "echo redis://host.docker.internal:6379"
inject = "env:REDIS_URL"

# --- Ports ---

[ports]
web = 3000
postgres = 5432
redis = 6379

# --- Assign: branch-switch behavior ---

[assign]
default = "none"
exclude_paths = ["docs", ".github", "scripts"]

[assign.services]
web = "hot"
worker = "hot"

[assign.rebuild_triggers]
web = ["package.json", "yarn.lock"]
worker = ["package.json", "yarn.lock"]
```

## Проект и настройка

```toml
[coast]
name = "my-nextjs-app"
primary_port = "web"
private_paths = ["packages/web/.next"]
worktree_dir = [".worktrees", ".claude/worktrees"]
```

**`private_paths`** критически важен для Next.js. Turbopack создаёт lock-файл в `.next/dev/lock` при запуске. Без `private_paths` второй экземпляр Coast на той же ветке увидит этот lock и откажется запускаться. С ним каждый экземпляр получает собственную изолированную директорию `.next` через overlay mount для каждого экземпляра. См. [Private Paths](../concepts_and_terminology/PRIVATE_PATHS.md).

**`worktree_dir`** перечисляет директории, где находятся git worktree. Если вы используете несколько coding agents (Claude Code, Cursor, Codex), каждый из них может создавать worktree в разных местах. Указание их всех позволяет Coast находить и назначать worktree независимо от того, каким инструментом они были созданы.

```toml
[coast.setup]
packages = ["nodejs", "npm", "make", "git", "bash"]
run = [
    "npm install -g corepack",
    "corepack enable",
]
```

Раздел setup устанавливает системные пакеты и инструменты, необходимые bare services. `corepack enable` активирует yarn или pnpm на основе поля `packageManager` проекта. Эти команды выполняются во время сборки внутри образа Coast, а не при запуске экземпляра.

## Bare Services

```toml
[services.web]
install = [
    "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)",
    "cd /workspace && test -f config.json || echo {} > config.json",
    "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn prisma migrate dev",
]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev"
port = 3000
restart = "on-failure"
cache = ["node_modules"]
```

**Условная установка:** Шаблон `test -f node_modules/.yarn-state.yml || make yarn` пропускает установку зависимостей, если `node_modules` уже существует. Это делает переключение веток быстрым, когда зависимости не изменились. См. [Bare Service Optimization](../concepts_and_terminology/BARE_SERVICE_OPTIMIZATION.md).

**`cache`:** Сохраняет `node_modules` при переключении worktree, чтобы `yarn install` выполнялся инкрементально, а не с нуля.

**`AUTH_URL` с динамическим портом:** Приложения Next.js, использующие NextAuth (или похожие библиотеки аутентификации), встраивают callback URL в ответы. Внутри Coast Next.js слушает порт 3000, но порт на стороне хоста является динамическим. Coast автоматически внедряет `WEB_DYNAMIC_PORT` в окружение контейнера (производится из ключа `web` в `[ports]`). Fallback `:-3000` означает, что та же команда работает и вне Coast. См. [Dynamic Port Environment Variables](../concepts_and_terminology/DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md).

**`host.docker.internal`:** Bare services не могут обращаться к shared services через `localhost`, потому что shared services работают в Docker daemon хоста. `host.docker.internal` разрешается в адрес хоста изнутри контейнера Coast.

## Shared Services

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["myapp_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_DB = "postgres", POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "postgres" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["myapp_redis_data:/data"]
```

Postgres и Redis запускаются в Docker daemon хоста как [shared services](../concepts_and_terminology/SHARED_SERVICES.md). Каждый экземпляр Coast подключается к одним и тем же базам данных, поэтому пользователи, сессии и данные разделяются между экземплярами. Это позволяет избежать проблемы, когда нужно регистрироваться отдельно в каждом экземпляре.

Если в вашем проекте уже есть `docker-compose.yml` с Postgres и Redis, вы можете использовать `compose` вместо этого и установить стратегию томов в `shared`. Shared services проще для Coastfile с bare services, потому что не нужно управлять compose-файлом.

## Секреты

```toml
[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"

[secrets.redis_url]
extractor = "command"
run = "echo redis://host.docker.internal:6379"
inject = "env:REDIS_URL"
```

Они внедряют `DATABASE_URL` и `REDIS_URL` в окружение контейнера Coast во время сборки. Строки подключения указывают на shared services через `host.docker.internal`.

Extractor `command` выполняет shell-команду и захватывает stdout. Здесь он просто выводит статическую строку, но его также можно использовать для чтения из vault, запуска CLI-инструмента или динамического вычисления значения.

Обратите внимание, что поля `command` bare service также задают эти переменные inline. Inline-значения имеют приоритет, но внедрённые секреты служат значениями по умолчанию для шагов `install` и сессий `coast exec`.

## Стратегии Assign

```toml
[assign]
default = "none"
exclude_paths = ["docs", ".github", "scripts"]

[assign.services]
web = "hot"
worker = "hot"

[assign.rebuild_triggers]
web = ["package.json", "yarn.lock"]
worker = ["package.json", "yarn.lock"]
```

**`default = "none"`** оставляет shared services и инфраструктуру нетронутыми при переключении ветки. Стратегия assign нужна только для сервисов, зависящих от кода.

**`hot` для Next.js и воркеров:** У Next.js с Turbopack есть встроенная hot module replacement. Когда Coast перемонтирует `/workspace` на новый worktree, Turbopack обнаруживает изменения файлов и автоматически перекомпилирует проект. Перезапуск процесса не нужен. Фоновые воркеры, использующие `tsc --watch` или `nodemon`, также подхватывают изменения через свои file watcher.

**`rebuild_triggers`:** Если `package.json` или `yarn.lock` изменились между ветками, команды `install` сервиса будут выполнены повторно перед перезапуском сервиса. Это гарантирует, что зависимости актуальны после переключения ветки, в которой были добавлены или удалены пакеты.

**`exclude_paths`:** Ускоряет первую инициализацию worktree, пропуская директории, которые сервисам не нужны. Документацию, конфигурации CI и скрипты можно безопасно исключить.

## Адаптация этого рецепта

**Без фонового воркера:** Удалите секцию `[services.worker]` и её запись в assign. Остальная часть Coastfile будет работать без изменений.

**Monorepo с несколькими приложениями Next.js:** Добавьте запись `private_paths` для директории `.next` каждого приложения. Каждый bare service получает собственную секцию `[services.*]` с соответствующими `command` и `port`.

**pnpm вместо yarn:** Замените `make yarn` на вашу команду установки pnpm. При необходимости скорректируйте поле `cache`, если pnpm хранит зависимости в другом месте (например, `.pnpm-store`).

**Без shared services:** Если вы предпочитаете отдельные базы данных для каждого экземпляра, удалите секции `[shared_services]` и `[secrets]`. Добавьте Postgres и Redis в `docker-compose.yml`, задайте `compose` в секции `[coast]` и используйте [стратегии томов](../coastfiles/VOLUMES.md) для управления изоляцией. Используйте `strategy = "isolated"` для данных на экземпляр или `strategy = "shared"` для общих данных.

**Дополнительные providers аутентификации:** Если ваша библиотека аутентификации использует переменные окружения, отличные от `AUTH_URL`, для callback URL, примените тот же шаблон `${WEB_DYNAMIC_PORT:-3000}` к этим переменным в команде сервиса.
