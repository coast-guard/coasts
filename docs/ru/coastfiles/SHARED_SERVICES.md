# Общие сервисы

Разделы `[shared_services.*]` определяют инфраструктурные сервисы — базы данных, кэши, брокеры сообщений — которые запускаются на хостовом Docker daemon, а не внутри отдельных контейнеров Coast. Несколько инстансов Coast подключаются к одному и тому же общему сервису через bridge-сеть.

О том, как общие сервисы работают во время выполнения, об управлении жизненным циклом и устранении неполадок см. [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md).

## Определение общего сервиса

Каждый общий сервис — это именованный TOML-раздел внутри `[shared_services]`. Поле `image` обязательно; всё остальное — опционально.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image` (обязательно)

Docker-образ, который нужно запустить на хостовом daemon.

### `ports`

Список портов, которые сервис экспонирует. Используется для маршрутизации в bridge-сети между общим сервисом и инстансами Coast.

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

Значения портов должны быть ненулевыми.

### `volumes`

Строки привязки Docker volume (bind) для сохранения данных. Это Docker volumes на уровне хоста, а не volumes, управляемые Coast.

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
```

### `env`

Переменные окружения, передаваемые в контейнер сервиса.

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_DB = "mydb" }
```

### `auto_create_db`

Если `true`, Coast автоматически создаёт для каждого инстанса Coast отдельную базу данных внутри общего сервиса. По умолчанию `false`.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
auto_create_db = true
```

### `inject`

Внедряет информацию о подключении к общему сервису в инстансы Coast в виде переменной окружения или файла. Использует тот же формат `env:NAME` или `file:/path`, что и [secrets](SECRETS.md).

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
inject = "env:DATABASE_URL"
```

## Жизненный цикл

Общие сервисы автоматически запускаются, когда запускается первый инстанс Coast, который на них ссылается. Они продолжают работать после `coast stop` и `coast rm` — удаление инстанса не влияет на данные общего сервиса. Только `coast shared rm` останавливает и удаляет общий сервис.

Базы данных, созданные на инстанс с помощью `auto_create_db`, также переживают удаление инстанса. Используйте `coast shared db drop`, чтобы удалить их явно.

## Когда использовать общие сервисы vs volumes

Используйте общие сервисы, когда нескольким инстансам Coast нужно обращаться к одному и тому же серверу БД (например, общий Postgres, где каждый инстанс получает свою отдельную базу). Используйте [стратегии volumes](VOLUMES.md), когда вы хотите контролировать, как данные compose-внутреннего сервиса совместно используются или изолируются.

## Примеры

### Postgres, Redis и MongoDB

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

### Минимальный общий Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

### Общие сервисы с автоматически создаваемыми базами данных

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```
