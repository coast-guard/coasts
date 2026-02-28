# Наследование, типы и композиция

Coastfile поддерживают наследование (`extends`), композицию фрагментов (`includes`), удаление элементов (`[unset]`) и исключение на уровне compose (`[omit]`). Вместе это позволяет один раз определить базовую конфигурацию и создавать лёгкие варианты под разные рабочие процессы — тест-раннеры, облегчённые фронтенды, стеки с предзаполненными снапшотами — без дублирования конфигурации.

Более высокоуровневый обзор того, как типизированные Coastfile вписываются в систему сборки, см. в [Coastfile Types](../concepts_and_terminology/COASTFILE_TYPES.md) и [Builds](../concepts_and_terminology/BUILDS.md).

## Типы Coastfile

Базовый Coastfile всегда называется `Coastfile`. Типизированные варианты используют шаблон имени `Coastfile.{type}`:

- `Coastfile` — тип по умолчанию
- `Coastfile.light` — тип `light`
- `Coastfile.snap` — тип `snap`
- `Coastfile.ci.minimal` — тип `ci.minimal`

Имя `Coastfile.default` зарезервировано и не допускается. Завершающая точка (`Coastfile.`) также недопустима.

Собирайте и запускайте типизированные варианты с `--type`:

```
coast build --type light
coast run test-1 --type light
```

У каждого типа свой независимый пул сборок. Сборка с `--type light` не влияет на сборки типа по умолчанию.

## `extends`

Типизированный Coastfile может наследоваться от родителя с помощью `extends` в секции `[coast]`. Сначала полностью разбирается родитель, затем значения ребёнка накладываются поверх.

```toml
[coast]
extends = "Coastfile"
```

Значение — это относительный путь к родительскому Coastfile, вычисляемый относительно директории ребёнка. Поддерживаются цепочки — ребёнок может расширять родителя, который сам расширяет «дедушку»:

```
Coastfile                    (base)
  └─ Coastfile.light         (extends Coastfile)
       └─ Coastfile.chain    (extends Coastfile.light)
```

Циклические цепочки (A extends B extends A или A extends A) обнаруживаются и отклоняются.

### Семантика слияния

Когда ребёнок расширяет родителя:

- **Скалярные поля** (`name`, `runtime`, `compose`, `root`, `worktree_dir`, `autostart`, `primary_port`) — если значение задано у ребёнка, оно побеждает; иначе наследуется от родителя.
- **Карты** (`[ports]`, `[egress]`) — сливаются по ключу. Ключи ребёнка переопределяют одноимённые ключи родителя; ключи, существующие только у родителя, сохраняются.
- **Именованные секции** (`[secrets.*]`, `[volumes.*]`, `[shared_services.*]`, `[mcp.*]`, `[mcp_clients.*]`, `[services.*]`) — сливаются по имени. Запись ребёнка с тем же именем полностью заменяет запись родителя; новые имена добавляются.
- **`[coast.setup]`**:
  - `packages` — объединение с дедупликацией (ребёнок добавляет новые пакеты, пакеты родителя сохраняются)
  - `run` — команды ребёнка добавляются после команд родителя
  - `files` — сливаются по `path` (одинаковый path = запись ребёнка заменяет запись родителя)
- **`[inject]`** — списки `env` и `files` конкатенируются.
- **`[omit]`** — списки `services` и `volumes` конкатенируются.
- **`[assign]`** — полностью заменяется, если присутствует у ребёнка (не сливается по полям).
- **`[agent_shell]`** — полностью заменяется, если присутствует у ребёнка.

### Наследование имени проекта

Если ребёнок не задаёт `name`, он наследует имя родителя. Это нормально для типизированных вариантов — это варианты одного и того же проекта:

```toml
# Coastfile
[coast]
name = "my-app"
```

```toml
# Coastfile.light — наследует name "my-app"
[coast]
extends = "Coastfile"
autostart = false
```

Вы можете переопределить `name` у ребёнка, если хотите, чтобы вариант отображался как отдельный проект:

```toml
[coast]
extends = "Coastfile"
name = "my-app-light"
```

## `includes`

Поле `includes` сливает один или несколько TOML-файлов-фрагментов в Coastfile до применения собственных значений файла. Это полезно для вынесения общей конфигурации (например, набора секретов или MCP-серверов) в переиспользуемые фрагменты.

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]
```

Подключаемый фрагмент — это TOML-файл с той же структурой секций, что и Coastfile. Он должен содержать секцию `[coast]` (она может быть пустой), но не может использовать `extends` или `includes` внутри себя.

```toml
# extra-secrets.toml
[coast]

[secrets.mongo_uri]
extractor = "env"
var = "MONGO_URI"
inject = "env:MONGO_URI"
```

Порядок слияния, когда присутствуют и `extends`, и `includes`:

1. Разобрать родителя (через `extends`) рекурсивно
2. Слить каждый подключённый фрагмент по порядку
3. Применить собственные значения файла (они побеждают всё)

## `[unset]`

Удаляет именованные элементы из разрешённой конфигурации после завершения всех слияний. Так ребёнок удаляет то, что унаследовал от родителя, без необходимости переопределять всю секцию целиком.

```toml
[unset]
secrets = ["db_password"]
shared_services = ["postgres", "redis"]
ports = ["postgres", "redis"]
```

Поддерживаемые поля:

- `secrets` — список имён секретов для удаления
- `ports` — список имён портов для удаления
- `shared_services` — список имён общих сервисов для удаления
- `volumes` — список имён томов для удаления
- `mcp` — список имён MCP-серверов для удаления
- `mcp_clients` — список имён MCP-клиентов для удаления
- `egress` — список имён egress для удаления
- `services` — список имён обычных сервисов для удаления

`[unset]` применяется после полного разрешения цепочки слияний extends + includes. Он удаляет элементы по имени из итогового слитого результата.

## `[omit]`

Исключает compose-сервисы и тома из стека Docker Compose, который запускается внутри Coast. В отличие от `[unset]` (который удаляет конфигурацию уровня Coastfile), `[omit]` говорит Coast исключить конкретные сервисы или тома при запуске `docker compose up` внутри DinD-контейнера.

```toml
[omit]
services = ["monitoring", "debug-tools", "nginx-proxy"]
volumes = ["keycloak-db-data"]
```

- **`services`** — имена compose-сервисов, которые нужно исключить из `docker compose up`
- **`volumes`** — имена compose-томов, которые нужно исключить

Это полезно, когда ваш `docker-compose.yml` определяет сервисы, которые не нужны в каждом варианте Coast — стеки мониторинга, reverse proxy, админ-инструменты. Вместо поддержки нескольких compose-файлов вы используете один compose-файл и исключаете ненужное для каждого варианта.

Когда ребёнок расширяет родителя, списки `[omit]` конкатенируются — ребёнок добавляет к списку omit родителя.

## Примеры

### Облегчённый вариант для тестов

Расширяет базовый Coastfile, отключает автозапуск, исключает общие сервисы и запускает базы данных изолированно для каждого инстанса:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "backend", "postgres", "redis"]
shared_services = ["postgres", "redis", "mongodb"]

[omit]
services = ["redis", "backend", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[assign]
default = "none"
[assign.services]
backend-test = "rebuild"
migrations = "rebuild"
```

### Вариант с предзаполнением из снапшота

Удаляет общие сервисы из базы и заменяет их изолированными томами, инициализируемыми из снапшотов:

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

### Типизированный вариант с дополнительными общими сервисами и includes

Расширяет базовый, добавляет MongoDB и подтягивает дополнительные секреты из фрагмента:

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]

[ports]
mongodb = 37017

[shared_services.mongodb]
image = "mongo:7"
ports = [27017]
env = { MONGO_INITDB_ROOT_USERNAME = "dev", MONGO_INITDB_ROOT_PASSWORD = "dev" }

[omit]
services = ["debug-tools"]
```

### Многоуровневая цепочка наследования

Три уровня: base -> light -> chain.

```toml
# Coastfile.chain
[coast]
extends = "Coastfile.light"

[coast.setup]
run = ["echo 'chain setup appended'"]

[ports]
debug = 39999
```

Разрешённая конфигурация начинается с базового `Coastfile`, поверх него сливается `Coastfile.light`, затем поверх этого — `Coastfile.chain`. Команды setup `run` со всех трёх уровней конкатенируются по порядку. Setup `packages` дедуплицируются на всех уровнях.

### Исключение сервисов из большого compose-стека

Исключите сервисы из `docker-compose.yml`, которые не нужны для разработки:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[omit]
services = ["backend-debug", "backend-debug-test", "asynqmon", "postgres-keycloak", "keycloak", "redash-db-init", "redash-init", "redash", "redash-scheduler", "redash-worker", "langfuse-db-init", "langfuse", "nginx-proxy"]
volumes = ["keycloak-db-data"]

[ports]
web = 3000
backend = 8080
```
