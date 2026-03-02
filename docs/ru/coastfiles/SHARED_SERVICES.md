# Общие сервисы

Разделы `[shared_services.*]` определяют инфраструктурные сервисы — базы данных, кэши, брокеры сообщений — которые запускаются на хостовом демоне Docker, а не внутри отдельных контейнеров Coast. Несколько экземпляров Coast подключаются к одному и тому же общему сервису через bridge-сеть.

О том, как общие сервисы работают во время выполнения, об управлении жизненным циклом и об устранении неполадок см. [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md).

## Определение общего сервиса

Каждый общий сервис — это именованный TOML-раздел под `[shared_services]`. Поле `image` обязательно; всё остальное — опционально.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image` (обязательно)

Docker-образ, который нужно запускать на хостовом демоне.

### `ports`

Список портов, которые сервис публикует. Используется для маршрутизации в bridge-сети между общим сервисом и экземплярами Coast.

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

Значения портов должны быть ненулевыми.

### `volumes`

Строки привязки (bind) Docker-томов для сохранения данных. Это Docker-тома на уровне хоста, а не тома, управляемые Coast.

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

Если `true`, Coast автоматически создаёт базу данных для каждого экземпляра внутри общего сервиса для каждого экземпляра Coast. По умолчанию `false`.

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

## Жизненный цикл

Общие сервисы запускаются автоматически, когда запускается первый экземпляр Coast, который на них ссылается. Они продолжают работать после `coast stop` и `coast rm` — удаление экземпляра не влияет на данные общего сервиса. Только `coast shared rm` останавливает и удаляет общий сервис.

Базы данных для отдельных экземпляров, созданные через `auto_create_db`, также сохраняются после удаления экземпляра. Используйте `coast shared-services rm`, чтобы удалить сервис и его данные целиком.

## Когда использовать общие сервисы vs тома

Используйте общие сервисы, когда нескольким экземплярам Coast нужно подключаться к одному и тому же серверу базы данных (например, общий Postgres, где каждый экземпляр получает свою собственную базу данных). Используйте [стратегии томов](VOLUMES.md), когда вы хотите управлять тем, как данные compose-внутреннего сервиса разделяются или изолируются.

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
