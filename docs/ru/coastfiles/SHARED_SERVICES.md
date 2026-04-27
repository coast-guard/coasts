# Общие сервисы

Разделы `[shared_services.*]` определяют инфраструктурные сервисы — базы данных, кэши, брокеры сообщений — которые проект Coast использует. Есть два варианта:

- **Inline** — объявите `image`, `ports`, `env`, `volumes` прямо в потребляющем Coastfile. Coast запускает контейнер на стороне хоста и направляет к нему трафик приложения потребителя. Лучше всего подходит для одиночных проектов с одним экземпляром-потребителем или для очень лёгких сервисов.
- **Из Shared Service Group (`from_group = true`)** — сервис живёт в [Shared Service Group](../shared_service_groups/README.md) проекта (отдельный DinD-контейнер, объявленный в `Coastfile.shared_service_groups`). Потребляющий Coastfile только подключается к нему. Лучше всего подходит, когда вам нужны извлечение секретов, checkout на стороне хоста к каноническим портам, или когда вы запускаете на этом хосте несколько проектов Coast, каждому из которых нужен один и тот же канонический порт (SSG удерживает Postgres на внутреннем `:5432`, не привязывая host 5432, поэтому два проекта могут сосуществовать).

Две половины этой страницы по очереди документируют каждый из этих вариантов.

О том, как общие сервисы работают во время выполнения, об управлении жизненным циклом и об устранении неполадок см. [Shared Services (concept)](../concepts_and_terminology/SHARED_SERVICES.md).

---

## Inline shared services

Каждый inline-сервис — это именованный TOML-раздел под `[shared_services]`. Поле `image` обязательно; всё остальное опционально.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image` (обязательно)

Docker-образ, который нужно запускать на хостовом демоне.

### `ports`

Список портов, которые сервис открывает. Coast принимает либо просто порты контейнера, либо сопоставления в стиле Docker Compose `"HOST:CONTAINER"`.

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- Простое целое число, например `6379`, является сокращением для `"6379:6379"`.
- Строка сопоставления, например `"5433:5432"`, публикует общий сервис на порту хоста `5433`, сохраняя при этом доступ к нему внутри Coast по адресу `service-name:5432`.
- И порт хоста, и порт контейнера должны быть ненулевыми.

### `volumes`

Строки привязки Docker-томов для сохранения данных. Это Docker-тома на уровне хоста, а не тома, управляемые Coast.

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
```

### `env`

Переменные окружения, передаваемые контейнеру сервиса.

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_DB = "mydb" }
```

### `auto_create_db`

Если `true`, Coast автоматически создаёт отдельную базу данных внутри общего сервиса для каждого экземпляра Coast. По умолчанию `false`.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
auto_create_db = true
```

### `inject`

Внедряет информацию о подключении к общему сервису в экземпляры Coast в виде переменной окружения или файла. Использует тот же формат `env:NAME` или `file:/path`, что и [secrets](SECRETS.md).

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
inject = "env:DATABASE_URL"
```

### Lifecycle

Inline-общие сервисы запускаются автоматически, когда запускается первый экземпляр Coast, который на них ссылается. Они продолжают работать после `coast stop` и `coast rm` — удаление экземпляра не влияет на данные общего сервиса. Только `coast shared rm` останавливает и удаляет сервис.

Базы данных для отдельных экземпляров, созданные через `auto_create_db`, также сохраняются после удаления экземпляра. Используйте `coast shared-services rm`, чтобы удалить сервис и его данные целиком.

### Inline examples

#### Postgres, Redis, and MongoDB

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_MULTIPLE_DATABASES = "dev_db,test_db" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis_data:/data"]

[shared_services.mongodb]
image = "mongo:latest"
ports = [27017]
volumes = ["infra_mongodb_data:/data/db"]
env = { MONGO_INITDB_ROOT_USERNAME = "myapp", MONGO_INITDB_ROOT_PASSWORD = "myapp_pass" }
```

#### Minimal shared Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### Host/container mapped Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = ["5433:5432"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### Auto-created databases

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

---

## Shared services from a Shared Service Group

Для проектов, которым нужна структурированная конфигурация общей инфраструктуры — несколько worktree, checkout на стороне хоста, SSG-native secrets, виртуальные порты между пересборками SSG — объявите сервисы один раз в [`Coastfile.shared_service_groups`](SHARED_SERVICE_GROUPS.md) и ссылайтесь на них из потребляющего Coastfile с помощью `from_group = true`:

```toml
[shared_services.postgres]
from_group = true

# Optional per-consumer overrides:
inject = "env:DATABASE_URL"
# auto_create_db = false    # overrides the SSG service's default
```

Ключ TOML (`postgres` в этом примере) должен совпадать с сервисом, объявленным в `Coastfile.shared_service_groups` проекта. SSG, на который здесь идёт ссылка, **всегда является собственным SSG проекта-потребителя** (с именем `<project>-ssg`, где `<project>` — это `[coast].name` потребителя).

### Forbidden fields with `from_group = true`

Следующие поля отклоняются во время разбора, потому что SSG — единственный источник истины:

- `image`
- `ports`
- `env`
- `volumes`

Любое из этих полей вместе с `from_group = true` приводит к:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports, env, volumes.
```

### Allowed per-consumer overrides

- `inject` — переменная окружения или путь к файлу, через которые предоставляется строка подключения. Разные потребляющие Coastfile могут предоставлять один и тот же SSG Postgres под разными именами переменных окружения.
- `auto_create_db` — должен ли Coast создавать отдельную базу данных внутри этого сервиса во время `coast run`. Переопределяет собственное значение `auto_create_db` сервиса SSG.

### Missing-service error

Если вы ссылаетесь на имя, которое не объявлено в `Coastfile.shared_service_groups` проекта, `coast build` завершится ошибкой:

```text
error: shared service 'postgres' has from_group = true but no service named 'postgres' is declared in Coastfile.shared_service_groups for project 'my-app'.
```

### When to choose `from_group` over inline

| Need | Inline | `from_group` |
|---|---|---|
| Single Coast project on this host, no secrets | Подойдёт любой вариант; inline проще | OK |
| Multiple worktrees / consumer instances of the **same** project sharing one Postgres | Работает (соседние экземпляры используют один host-контейнер) | Работает |
| **Two different Coast projects** on this host that each declare the same canonical port (e.g. both want Postgres on 5432) | Конфликтуют из-за host-порта; нельзя запускать оба одновременно | Обязательно (SSG каждого проекта владеет своим внутренним Postgres без привязки host 5432) |
| Want host-side `psql localhost:5432` via `coast ssg checkout` | -- | Обязательно |
| Need build-time secret extraction for the service (`POSTGRES_PASSWORD` from a keychain, etc.) | -- | Обязательно (см. [SSG Secrets](../shared_service_groups/SECRETS.md)) |
| Stable consumer routing across rebuilds (virtual ports) | -- | Обязательно (см. [SSG Routing](../shared_service_groups/ROUTING.md)) |

Полное описание архитектуры SSG см. в [Shared Service Groups](../shared_service_groups/README.md). Об опыте на стороне потребителя, включая автозапуск, обнаружение дрейфа и удалённых потребителей, см. [Consuming](../shared_service_groups/CONSUMING.md).

---

## See Also

- [Shared Services (concept)](../concepts_and_terminology/SHARED_SERVICES.md) -- архитектура времени выполнения для обоих вариантов
- [Shared Service Groups](../shared_service_groups/README.md) -- обзор концепции SSG
- [Coastfile: Shared Service Groups](SHARED_SERVICE_GROUPS.md) -- схема Coastfile для стороны SSG
- [Consuming an SSG](../shared_service_groups/CONSUMING.md) -- подробное описание семантики `from_group = true`
