# Full-Stack Monorepo

Этот рецепт предназначен для большого монорепозитория с несколькими веб‑приложениями, использующими общие базу данных и слой кэша. Стек использует Docker Compose для тяжёлых бэкенд‑сервисов (Rails, Sidekiq, SSR) и запускает dev‑серверы Vite как bare‑сервисы на хосте DinD. Postgres и Redis запускаются как общие сервисы на хостовом Docker‑демоне, поэтому каждый экземпляр Coast обращается к одной и той же инфраструктуре, не дублируя её.

Этот паттерн хорошо подходит, когда:

- Ваш монорепозиторий содержит несколько приложений, которые разделяют одну базу данных
- Вы хотите лёгкие экземпляры Coast, которые не запускают каждый свой собственный Postgres и Redis
- Ваши dev‑серверы фронтенда должны быть доступны из compose‑контейнеров через `host.docker.internal`
- У вас есть host‑side интеграции MCP, которые подключаются к `localhost:5432`, и вы хотите, чтобы они продолжали работать без изменений

## The Complete Coastfile

Вот полный Coastfile. Каждый раздел подробно объясняется ниже.

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

Поле `compose` указывает на ваш существующий Docker Compose файл. Coast запускает `docker compose up -d` внутри контейнера DinD при `coast run`, поэтому ваши бэкенд‑сервисы (Rails‑серверы, воркеры Sidekiq, SSR‑процессы) стартуют автоматически.

`[coast.setup]` устанавливает пакеты на самом хосте DinD — не внутри compose‑контейнеров. Они нужны bare‑сервисам (dev‑серверам Vite), которые запускаются напрямую на хосте. Compose‑сервисы, как обычно, получают свои рантаймы из Dockerfile.

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

Postgres и Redis объявлены как [shared services](../concepts_and_terminology/SHARED_SERVICES.md), а не запускаются внутри каждого Coast. Это означает, что они работают на хостовом Docker‑демоне, а каждый экземпляр Coast подключается к ним через bridge‑сеть.

**Почему shared services вместо баз данных внутри compose?**

- **Более лёгкие экземпляры.** Каждый Coast не поднимает собственные контейнеры Postgres и Redis, что экономит память и ускоряет старт.
- **Повторное использование host volume.** Поле `volumes` ссылается на ваши существующие Docker‑тома (те, что созданы вашим локальным `docker-compose up`). Все уже имеющиеся данные сразу доступны — без сидирования и повторных прогонов миграций.
- **Совместимость с MCP.** Если у вас на хосте есть MCP‑инструменты для базы, подключающиеся к `localhost:5432`, они продолжают работать, потому что общий Postgres находится на хосте на том же порту. Перенастройка не требуется.

**Компромисс:** между экземплярами Coast нет изоляции данных. Каждый экземпляр читает и пишет в одну и ту же базу. Если вашему процессу нужны базы на каждый экземпляр, используйте [volume strategies](../concepts_and_terminology/VOLUMES.md) с `strategy = "isolated"` вместо этого, или используйте `auto_create_db = true` на shared‑сервисе, чтобы получить базу на каждый экземпляр внутри общего Postgres. Подробности — в [Shared Services Coastfile reference](../coastfiles/SHARED_SERVICES.md).

**Имена томов важны.** Имена томов (`infra_postgres`, `infra_redis`) должны совпадать с томами, которые уже существуют на вашем хосте после запуска `docker-compose up` локально. Если они не совпадают, shared‑сервис стартует с пустым томом. Выполните `docker volume ls`, чтобы проверить существующие имена томов перед тем, как писать этот раздел.

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

Dev‑серверы Vite определены как [bare services](../concepts_and_terminology/BARE_SERVICES.md) — обычные процессы, запущенные напрямую на хосте DinD, вне Docker Compose. Это паттерн [mixed service types](../concepts_and_terminology/MIXED_SERVICE_TYPES.md).

**Почему bare вместо compose?**

Главная причина — сеть. Compose‑сервисы, которым нужно достучаться до dev‑сервера Vite (для SSR, проксирования ассетов или HMR WebSocket‑соединений), могут использовать `host.docker.internal`, чтобы обратиться к bare‑сервисам на хосте DinD. Это позволяет избежать сложной настройки Docker‑сетей и соответствует тому, как большинство монорепо‑настроек конфигурируют `VITE_RUBY_HOST` или похожие переменные окружения.

Bare‑сервисы также получают прямой доступ к bind-mounted файловой системе `/workspace` без прохождения через overlay внутреннего контейнера. Это означает, что file watcher Vite быстрее реагирует на изменения.

**`install` и `cache`:** Поле `install` выполняется перед стартом сервиса и снова при каждом `coast assign`. Здесь оно запускает `yarn install`, чтобы подхватывать изменения зависимостей при переключении веток. Поле `cache` говорит Coast сохранять `node_modules` при переключениях worktree, чтобы установки были инкрементальными, а не «с нуля».

**Только один `install`:** Обратите внимание, что у `vite-api` нет поля `install`. В монорепозитории с yarn workspaces одного `yarn install` в корне достаточно, чтобы установить зависимости для всех workspace. Размещая его только на одном сервисе, вы избегаете двойного запуска.

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

Каждый порт, которым должен управлять Coast, задаётся в `[ports]`. Каждый экземпляр получает [dynamic port](../concepts_and_terminology/PORTS.md) (высокий диапазон, всегда доступен) для каждого объявленного порта. Экземпляр [checked-out](../concepts_and_terminology/CHECKOUT.md) также получает проброс канонического порта (число, которое вы объявили) на хост.

Раздел `[healthcheck]` говорит Coast, как проверять здоровье каждого порта. Для портов, у которых настроен путь healthcheck, Coast отправляет HTTP GET каждые 5 секунд — любой HTTP‑ответ считается признаком здоровья. Для портов без пути healthcheck используется TCP‑проверка подключения (может ли порт принять соединение?).

В этом примере Rails web‑серверы получают HTTP‑проверки по `/` потому что они отдают HTML‑страницы. Для dev‑серверов Vite пути healthcheck не задаются — они не отдают осмысленную корневую страницу, и TCP‑проверки достаточно, чтобы понять, что они принимают соединения.

Статус healthcheck виден в UI [Coastguard](../concepts_and_terminology/COASTGUARD.md) и через `coast ports`.

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

Все тома здесь используют `strategy = "shared"`, что означает: один Docker‑том разделяется между всеми экземплярами Coast. Это правильный выбор для **кэшей и артефактов сборки** — вещей, где конкурентные записи безопасны, а дублирование «на каждый экземпляр» будет тратить место на диске и замедлять старт:

- **`bundle`** — кэш Ruby‑гемов. Гемы одинаковы между ветками. Совместное использование избавляет от повторного скачивания всего bundle для каждого экземпляра Coast.
- **`*_rails_cache`** — файловые кэши Rails. Они ускоряют разработку, но не являются ценными — любой экземпляр может их пересоздать.
- **`*_assets`** — скомпилированные ассеты. Та же логика, что и для кэшей.

**Почему не shared для баз данных?** Coast выводит предупреждение, если вы используете `strategy = "shared"` для тома, подключённого к сервису типа базы данных. Несколько процессов Postgres, пишущих в один и тот же каталог данных, приводят к порче данных. Для баз данных либо используйте [shared services](../coastfiles/SHARED_SERVICES.md) (один Postgres на хосте, как в этом рецепте), либо `strategy = "isolated"` (каждый Coast получает свой том). Полную матрицу решений смотрите на странице [Volume Topology](../concepts_and_terminology/VOLUMES.md).

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

Раздел `[assign]` управляет тем, что происходит с каждым сервисом, когда вы запускаете `coast assign`, чтобы переключить экземпляр Coast на другой worktree. Правильная настройка — это разница между переключением ветки за 5 секунд и за 60 секунд.

### `default = "none"`

Установка значения по умолчанию `"none"` означает, что любой сервис, не перечисленный явно в `[assign.services]`, при переключении ветки остаётся без изменений. Это критично для баз данных и кэшей — Postgres, Redis и инфраструктурные сервисы не меняются между ветками, и их перезапуск — лишняя работа.

### Стратегии по сервисам

| Service | Strategy | Why |
|---|---|---|
| `web-rails`, `web-ssr`, `api-rails` | `hot` | Они запускают dev‑серверы с file watcher'ами. [Filesystem remount](../concepts_and_terminology/FILESYSTEM.md) подменяет код под `/workspace`, и watcher автоматически подхватывает изменения. Перезапуск контейнера не нужен. |
| `web-sidekiq`, `api-sidekiq` | `restart` | Фоновые воркеры загружают код при старте и не следят за изменениями файлов. Им нужен перезапуск контейнера, чтобы подхватить код новой ветки. |

Перечисляйте только те сервисы, которые действительно запущены. Если ваши `COMPOSE_PROFILES` запускают лишь подмножество сервисов, не перечисляйте неактивные — Coast вычисляет стратегию assign для каждого перечисленного сервиса, и перезапуск сервиса, который не запущен, — лишняя работа. Подробнее — в [Performance Optimizations](../concepts_and_terminology/PERFORMANCE_OPTIMIZATIONS.md).

### `exclude_paths`

Это самая влиятельная оптимизация для больших монорепозиториев. Она говорит Coast пропускать целые деревья каталогов во время синхронизации файлов, отслеживаемых gitignored (rsync), и diff `git ls-files`, которые запускаются при каждом assign.

Цель — исключить всё, что не нужно вашим сервисам Coast. В монорепозитории на 30 000 файлов перечисленные выше каталоги могут давать 8 000+ файлов, не относящихся к работающим сервисам. Их исключение сокращает на столько же количество file stat'ов при каждом переключении ветки.

Чтобы понять, что исключать, профилируйте репозиторий:

```bash
git ls-files | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

Оставляйте каталоги, содержащие исходники, которые монтируются в запущенные сервисы, или общие библиотеки, импортируемые этими сервисами. Исключайте всё остальное — документацию, конфигурации CI, инструменты, приложения других команд, мобильные клиенты, CLI‑утилиты и vendored кэши вроде `.yarn`.

### `rebuild_triggers`

Без триггеров сервис со `strategy = "rebuild"` пересобирает Docker‑образ при каждом переключении ветки — даже если ничего, влияющего на образ, не изменилось. Раздел `[assign.rebuild_triggers]` ограничивает пересборку конкретными файлами.

В этом рецепте Rails‑сервисы обычно используют `"hot"` (вообще без перезапуска). Но если кто-то меняет Dockerfile или Gemfile, срабатывают `rebuild_triggers` и принудительно запускают полную пересборку образа. Если ни один из файлов‑триггеров не изменился, Coast полностью пропускает пересборку. Это избегает дорогих сборок образов при обычных изменениях кода и при этом ловит изменения на уровне инфраструктуры.

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

Раздел `[secrets]` извлекает значения во время сборки и внедряет их в экземпляры Coast как переменные окружения.

- **`compose_profiles`** управляет тем, какие профили Docker Compose запускаются. Так вы ограничиваете Coast запуском только профилей `api` и `web`, а не всех сервисов, определённых в compose‑файле. Переопределите это на хосте с `export COMPOSE_PROFILES=api,web,portal` перед сборкой, чтобы изменить набор запускаемых сервисов.
- **`uid` / `gid`** передают UID и GID пользователя хоста в контейнер — это распространённо в Docker‑настройках, где нужно, чтобы владельцы файлов совпадали между хостом и контейнером.

Раздел `[inject]` проще — он пробрасывает существующие переменные окружения хоста в контейнер Coast во время выполнения. Чувствительные учётные данные, такие как токены gem‑сервера (`BUNDLE_GEMS__CONTRIBSYS__COM`), остаются на хосте и пробрасываются без записи в какой-либо конфигурационный файл.

Полную справку по extractors для secrets и целям injection смотрите в [Secrets](../coastfiles/SECRETS.md).

## Adapting This Recipe

**Другой языковой стек:** Замените специфичные для Rails тома (bundle, rails cache, assets) на эквиваленты для вашего стека — кэш Go modules (`/go/pkg/mod`), кэш npm, кэш pip и т. д. Стратегия остаётся `"shared"` для любого кэша, который безопасно разделять между экземплярами.

**Меньше приложений:** Если в вашем монорепозитории только одно приложение, уберите лишние записи томов и упростите `[assign.services]`, перечислив только ваши сервисы. Паттерны shared services и bare services по-прежнему применимы.

**Базы данных на каждый экземпляр:** Если вам нужна изоляция данных между экземплярами Coast, замените `[shared_services.db]` на Postgres внутри compose и добавьте запись в `[volumes]` со `strategy = "isolated"`. Каждый экземпляр получит свой том базы данных. Вы можете засидировать его из тома хоста с помощью `snapshot_source` — см. [Volumes Coastfile reference](../coastfiles/VOLUMES.md).

**Без bare‑сервисов:** Если ваш фронтенд полностью контейнеризован и ему не нужно быть доступным через `host.docker.internal`, удалите секции `[services.*]` и `[coast.setup]`. Всё будет запускаться через compose.
