# Наследование, типы и композиция

Coastfile поддерживают наследование (`extends`), композицию фрагментов (`includes`), удаление элементов (`[unset]`) и исключение на уровне compose (`[omit]`). Вместе они позволяют один раз определить базовую конфигурацию и создавать компактные варианты для разных рабочих процессов — тестовых раннеров, облегчённых фронтендов, стеков с предзаполненными снапшотами — без дублирования конфигурации.

Для более общего обзора того, как типизированные Coastfile вписываются в систему сборки, см. [Coastfile Types](../concepts_and_terminology/COASTFILE_TYPES.md) и [Builds](../concepts_and_terminology/BUILDS.md).

## Типы Coastfile

Базовый Coastfile всегда называется `Coastfile`. Типизированные варианты используют шаблон имени `Coastfile.{type}`:

- `Coastfile` — тип по умолчанию
- `Coastfile.light` — тип `light`
- `Coastfile.snap` — тип `snap`
- `Coastfile.ci.minimal` — тип `ci.minimal`

Любой Coastfile может иметь необязательное расширение `.toml` для подсветки синтаксиса в редакторе. Суффикс `.toml` удаляется перед извлечением типа:

- `Coastfile.toml` = `Coastfile` (тип по умолчанию)
- `Coastfile.light.toml` = `Coastfile.light` (тип `light`)
- `Coastfile.ci.minimal.toml` = `Coastfile.ci.minimal` (тип `ci.minimal`)

Если существуют и обычная, и `.toml`-форма (например, `Coastfile` и `Coastfile.toml`), вариант `.toml` имеет приоритет.

Имена `Coastfile.default` и `"toml"` (как тип) зарезервированы и не допускаются. Завершающая точка (`Coastfile.`) также недопустима.

Собирайте и запускайте типизированные варианты с помощью `--type`:

```
coast build --type light
coast run test-1 --type light
```

У каждого типа есть собственный независимый пул сборок. Сборка с `--type light` не влияет на сборки по умолчанию.

## `extends`

Типизированный Coastfile может наследоваться от родительского с помощью `extends` в секции `[coast]`. Сначала полностью разбирается родитель, затем поверх него накладываются значения дочернего файла.

```toml
[coast]
extends = "Coastfile"
```

Значение — это относительный путь к родительскому Coastfile, который разрешается относительно директории дочернего файла. Если точный путь не существует, Coast также попробует добавить `.toml` — поэтому `extends = "Coastfile"` найдёт `Coastfile.toml`, если на диске существует только вариант `.toml`. Поддерживаются цепочки — дочерний файл может наследоваться от родителя, который сам наследуется от прародителя:

```
Coastfile                    (базовый)
  └─ Coastfile.light         (наследуется от Coastfile)
       └─ Coastfile.chain    (наследуется от Coastfile.light)
```

Циклические цепочки (A наследуется от B, который наследуется от A, или A наследуется от A) обнаруживаются и отклоняются.

### Семантика слияния

Когда дочерний файл наследуется от родительского:

- **Скалярные поля** (`name`, `runtime`, `compose`, `root`, `worktree_dir`, `autostart`, `primary_port`) — значение дочернего файла побеждает, если присутствует; иначе наследуется от родителя.
- **Отображения** (`[ports]`, `[egress]`) — сливаются по ключу. Ключи дочернего файла переопределяют одноимённые ключи родителя; ключи только из родителя сохраняются.
- **Именованные секции** (`[secrets.*]`, `[volumes.*]`, `[shared_services.*]`, `[mcp.*]`, `[mcp_clients.*]`, `[services.*]`) — сливаются по имени. Запись дочернего файла с тем же именем полностью заменяет запись родителя; новые имена добавляются.
- **`[coast.setup]`**:
  - `packages` — дедуплицированное объединение (дочерний файл добавляет новые пакеты, пакеты родителя сохраняются)
  - `run` — команды дочернего файла добавляются после команд родителя
  - `files` — сливаются по `path` (один и тот же путь = запись дочернего файла заменяет запись родителя)
- **`[inject]`** — списки `env` и `files` конкатенируются.
- **`[omit]`** — списки `services` и `volumes` конкатенируются.
- **`[assign]`** — полностью заменяется, если присутствует в дочернем файле (не сливается по полям).
- **`[agent_shell]`** — полностью заменяется, если присутствует в дочернем файле.

### Наследование имени проекта

Если дочерний файл не задаёт `name`, он наследует имя родителя. Это нормально для типизированных вариантов — они являются вариантами одного и того же проекта:

```toml
# Coastfile
[coast]
name = "my-app"
```

```toml
# Coastfile.light — наследует имя "my-app"
[coast]
extends = "Coastfile"
autostart = false
```

Вы можете переопределить `name` в дочернем файле, если хотите, чтобы вариант отображался как отдельный проект:

```toml
[coast]
extends = "Coastfile"
name = "my-app-light"
```

## `includes`

Поле `includes` сливает один или несколько TOML-фрагментов в Coastfile до применения собственных значений файла. Это полезно для вынесения общей конфигурации (например, набора секретов или MCP-серверов) в повторно используемые фрагменты.

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]
```

Включаемый фрагмент — это TOML-файл с такой же структурой секций, как у Coastfile. Он должен содержать секцию `[coast]` (которая может быть пустой), но не может сам использовать `extends` или `includes`.

```toml
# extra-secrets.toml
[coast]

[secrets.mongo_uri]
extractor = "env"
var = "MONGO_URI"
inject = "env:MONGO_URI"
```

Порядок слияния, когда присутствуют и `extends`, и `includes`:

1. Разобрать родительский файл (через `extends`) рекурсивно
2. Слить каждый включённый фрагмент по порядку
3. Применить собственные значения файла (они имеют приоритет над всем остальным)

## `[unset]`

Удаляет именованные элементы из результирующей конфигурации после завершения всех слияний. Так дочерний файл удаляет то, что унаследовал от родителя, без необходимости переопределять всю секцию целиком.

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
- `egress` — список имён egress-правил для удаления
- `services` — список имён обычных сервисов для удаления

`[unset]` применяется после того, как разрешена полная цепочка слияния extends + includes. Оно удаляет элементы по имени из окончательного объединённого результата.

## `[omit]`

Исключает compose-сервисы и тома из стека Docker Compose, который запускается внутри Coast. В отличие от `[unset]` (который удаляет конфигурацию на уровне Coastfile), `[omit]` указывает Coast исключить определённые сервисы или тома при запуске `docker compose up` внутри контейнера DinD.

```toml
[omit]
services = ["monitoring", "debug-tools", "nginx-proxy"]
volumes = ["keycloak-db-data"]
```

- **`services`** — имена compose-сервисов, которые нужно исключить из `docker compose up`
- **`volumes`** — имена compose-томов, которые нужно исключить

Это полезно, когда ваш `docker-compose.yml` определяет сервисы, которые не нужны в каждом варианте Coast — стеки мониторинга, обратные прокси, административные инструменты. Вместо поддержки нескольких compose-файлов вы используете один compose-файл и исключаете ненужное для каждого варианта.

Когда дочерний файл наследуется от родительского, списки `[omit]` конкатенируются — дочерний файл добавляет элементы в список исключений родителя.

## Примеры

### Облегчённый тестовый вариант

Наследуется от базового Coastfile, отключает autostart, убирает общие сервисы и запускает базы данных изолированно для каждого экземпляра:

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

Удаляет общие сервисы из базового файла и заменяет их изолированными томами, предзаполненными из снапшотов:

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

Наследуется от базового файла, добавляет MongoDB и подтягивает дополнительные секреты из фрагмента:

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

Три уровня глубины: базовый -> light -> chain.

```toml
# Coastfile.chain
[coast]
extends = "Coastfile.light"

[coast.setup]
run = ["echo 'chain setup appended'"]

[ports]
debug = 39999
```

Результирующая конфигурация начинается с базового `Coastfile`, затем поверх него сливается `Coastfile.light`, а затем поверх этого — `Coastfile.chain`. Команды `run` из `setup` со всех трёх уровней конкатенируются по порядку. `packages` из setup дедуплицируются на всех уровнях.

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
