# Coastfile.shared_service_groups

`Coastfile.shared_service_groups` — это типизированный Coastfile, который объявляет сервисы, запускаемые Shared Service Group (SSG) вашего проекта. Он располагается рядом с обычным `Coastfile`, а имя проекта берётся из `[coast].name` в этом соседнем файле — здесь его повторно указывать не нужно. У каждого проекта есть ровно один такой файл (в вашем worktree); контейнер `<project>-ssg` запускает объявленные в нём сервисы. Другие потребляющие Coastfile в том же проекте могут ссылаться на эти сервисы через `[shared_services.<name>] from_group = true`.

О концепции, жизненном цикле, томах, секретах и подключении потребителей см. в [документации по Shared Service Groups](../shared_service_groups/README.md).

## Discovery

`coast ssg build` находит файл по тем же правилам, что и `coast build`:

- По умолчанию: искать `Coastfile.shared_service_groups` или `Coastfile.shared_service_groups.toml` в текущем рабочем каталоге. Обе формы эквивалентны; вариант с `.toml` имеет приоритет, если существуют оба.
- `-f <path>` / `--file <path>` указывает на произвольный файл.
- `--working-dir <dir>` отделяет корень проекта от местоположения Coastfile.
- `--config '<toml>'` принимает встроенный TOML для сценарных потоков.

## Accepted Sections

Допускаются только `[ssg]`, `[shared_services.<name>]`, `[secrets.<name>]` и `[unset]`. Любой другой ключ верхнего уровня (`[coast]`, `[ports]`, `[services]`, `[volumes]`, `[assign]`, `[omit]`, `[inject]`, ...) отклоняется на этапе разбора.

Для композиции поддерживаются `[ssg] extends = "<path>"` и `[ssg] includes = ["<path>", ...]`. См. [Inheritance](#inheritance) ниже.

## `[ssg]`

Конфигурация SSG верхнего уровня.

```toml
[ssg]
runtime = "dind"
```

### `runtime` (optional)

Среда выполнения контейнера для внешнего SSG DinD. На данный момент поддерживается только значение `dind`; поле необязательно и по умолчанию равно `dind`.

## `[shared_services.<name>]`

Один блок на сервис. Ключ TOML (`postgres`, `redis`, ...) становится именем сервиса, на которое ссылаются потребляющие Coastfile.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

### `image` (required)

Docker-образ, который будет запущен внутри внутреннего Docker-демона SSG. Допускается любой публичный или приватный образ, который хост может скачать.

### `ports`

Порты контейнера, которые слушает сервис. **Только целые числа без дополнительного синтаксиса.**

```toml
ports = [5432]
ports = [5432, 5433]
```

- Отображение `"HOST:CONTAINER"` (`"5432:5432"`) **отклоняется**. Публикация портов хоста для SSG всегда динамическая — вы никогда не выбираете порт хоста вручную.
- Пустой массив (или полное отсутствие поля) допускается. Sidecar-сервисы без открытых портов допустимы.

Каждый порт во время `coast ssg run` становится отображением `PUBLISHED:CONTAINER` на внешнем DinD, где `PUBLISHED` — это динамически выделенный порт хоста. Для стабильной маршрутизации потребителей также выделяется отдельный виртуальный порт на уровне проекта — см. [Routing](../shared_service_groups/ROUTING.md).

### `env`

Плоская строковая map, дословно передаваемая в окружение внутреннего сервисного контейнера.

```toml
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "app" }
```

Значения env **не** сохраняются в манифесте сборки. Записываются только ключи, что соответствует политике безопасности `coast build`.

Для значений, которые вы не хотите хардкодить в Coastfile (пароли, API-токены), используйте секцию `[secrets.*]`, описанную ниже — она извлекает данные с вашего хоста во время сборки и внедряет их во время запуска.

### `volumes`

Массив строк томов в стиле Docker Compose. Каждый элемент имеет один из следующих видов:

```toml
volumes = [
    "/var/coast-data/postgres:/var/lib/postgresql/data",   # bind mount хоста
    "pg_wal:/var/lib/postgresql/wal",                       # именованный внутренний том
]
```

**Host bind mount** — источник начинается с `/`. Данные находятся в реальной файловой системе хоста. И внешний DinD, и внутренний сервис монтируют **одну и ту же строку пути хоста**. См. [Volumes -> Symmetric-Path Plan](../shared_service_groups/VOLUMES.md#the-symmetric-path-plan).

**Inner named volume** — источник является именем Docker volume (без `/`). Том находится внутри внутреннего Docker-демона SSG. Сохраняется между перезапусками SSG; непрозрачен для хоста.

Отклоняется на этапе разбора:

- Относительные пути (`./data:/...`).
- Компоненты `..`.
- Тома только для контейнера (без источника).
- Дублирующиеся target-пути в пределах одного сервиса.

### `auto_create_db`

Если `true`, демон создаёт базу данных `{instance}_{project}` внутри этого сервиса для каждого запускаемого потребляющего Coast. Применяется только к распознаваемым образам баз данных (Postgres, MySQL). По умолчанию `false`.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

Потребляющий Coastfile может переопределить это значение для конкретного проекта — см. [Consuming -> auto_create_db](../shared_service_groups/CONSUMING.md#auto_create_db).

### `inject` (not allowed)

`inject` **недопустим** в определениях сервисов SSG. Внедрение — это задача на стороне потребителя (разные потребляющие Coastfile могут хотеть, чтобы один и тот же SSG Postgres был доступен под разными именами переменных окружения). О семантике `inject` на стороне потребителя см. [Coastfile: Shared Services](SHARED_SERVICES.md#inject).

## `[secrets.<name>]`

Блок `[secrets.*]` в `Coastfile.shared_service_groups` извлекает учётные данные со стороны хоста во время `coast ssg build` и внедряет их во внутренние сервисы SSG во время `coast ssg run`. Схема отражает обычный `[secrets.*]` в Coastfile (список полей см. в [Secrets](SECRETS.md)); поведение, специфичное для SSG, задокументировано в [SSG Secrets](../shared_service_groups/SECRETS.md).

```toml
[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"

[secrets.tls_cert]
extractor = "file"
path = "/Users/me/certs/dev.pem"
inject = "file:/etc/ssl/certs/server.pem"
```

Доступны те же экстракторы (`env`, `file`, `command`, `keychain`, пользовательский `coast-extractor-<name>`). Директива `inject` определяет, попадёт ли значение как переменная окружения или как файл внутрь внутреннего сервисного контейнера SSG.

По умолчанию секрет, определённый нативно для SSG, внедряется в **каждый** объявленный `[shared_services.*]`. Чтобы ограничить его подмножеством, явно перечислите имена сервисов:

```toml
[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"
services = ["postgres"]      # монтируется только в сервис postgres
```

Извлечённые значения секретов хранятся в зашифрованном виде в `~/.coast/keystore.db` под `coast_image = "ssg:<project>"` — это пространство имён, отдельное от обычных записей keystore Coast. Полный жизненный цикл, включая команду `coast ssg secrets clear`, см. в [SSG Secrets](../shared_service_groups/SECRETS.md).

## Inheritance

SSG Coastfile поддерживают тот же механизм `extends` / `includes` / `[unset]`, что и обычные Coastfile. Общую ментальную модель см. в [Coastfile Inheritance](INHERITANCE.md); в этом разделе документируется форма, специфичная для SSG.

### `[ssg] extends` -- pull in a parent Coastfile

```toml
[ssg]
extends = "Coastfile.ssg-base"

[shared_services.postgres]
image = "postgres:17-alpine"
```

Родительский файл разрешается относительно родительского каталога дочернего файла. Применяется правило приоритета `.toml` (парсер сначала пробует `Coastfile.ssg-base.toml`, затем просто `Coastfile.ssg-base`). Абсолютные пути также допускаются.

### `[ssg] includes` -- merge fragment files

```toml
[ssg]
includes = ["dev-seed.toml", "extra-caches.toml"]

[shared_services.postgres]
image = "postgres:16-alpine"
```

Фрагменты объединяются по порядку до самого включающего файла. Пути к фрагментам разрешаются относительно родительского каталога включающего файла (без приоритета `.toml` — фрагменты обычно именуются точно).

**Сами фрагменты не могут использовать `extends` или `includes`.** Они должны быть самодостаточными.

### Merge semantics

- **Скалярные значения `[ssg]`** (`runtime`) — если у дочернего есть значение, побеждает оно, иначе наследуется.
- **`[shared_services.*]`** — замена по имени. Если и родитель, и дочерний файл определяют `postgres`, запись дочернего полностью заменяет запись родителя (замена всей записи, а не слияние на уровне полей). Родительские сервисы, не переобъявленные дочерним файлом, наследуются.
- **`[secrets.*]`** — замена по имени, по той же схеме, что и `[shared_services.*]`. Дочерний секрет с тем же именем полностью переопределяет конфигурацию секрета родителя.
- **Порядок загрузки** — сначала загружается родитель из `extends`, затем каждый фрагмент из `includes` по порядку, затем сам файл верхнего уровня. При конфликте побеждают более поздние слои.

### `[unset]` -- drop inherited services or secrets

```toml
[ssg]
extends = "Coastfile.ssg-base"

[unset]
shared_services = ["mongodb"]
secrets = ["pg_password"]
```

Удаляет именованные записи **после** слияния, так что дочерний файл может выборочно убрать что-то, предоставляемое родителем. Поддерживаются оба ключа: `shared_services` и `secrets`.

Технически самостоятельные SSG Coastfile могут содержать `[unset]`, но он молча игнорируется (это соответствует поведению обычного Coastfile: unset применяется только тогда, когда файл участвует в наследовании).

### Cycles

Прямые циклы (`A` extends `B` extends `A` или `A` extends сам себя) приводят к жёсткой ошибке `circular extends/includes dependency detected: '<path>'`. Ромбовидное наследование (две отдельные цепочки, которые обе заканчиваются одним и тем же родителем) допускается — набор посещённых узлов ведётся на уровне рекурсии и очищается при возврате.

### `[omit]` is not applicable

Обычные Coastfile поддерживают `[omit]` для удаления сервисов / томов из compose-файла. У SSG нет compose-файла, из которого можно что-то убрать — внутренний compose генерируется напрямую из записей `[shared_services.*]`. Вместо этого используйте `[unset]` для удаления унаследованных сервисов.

### Inline `--config` rejects `extends` / `includes`

`coast ssg build --config '<toml>'` не может разрешить путь к родителю, потому что нет местоположения на диске, относительно которого можно вычислять относительные пути. Передача `extends` / `includes` во встроенном TOML приводит к жёсткой ошибке `extends and includes require file-based parsing`. Вместо этого используйте `-f <file>` или `--working-dir <dir>`.

### Build artifact is the flattened form

`coast ssg build` записывает автономный TOML в `~/.coast/ssg/<project>/builds/<id>/ssg-coastfile.toml`. Артефакт содержит результат после слияния с учётом наследования без директив `extends`, `includes` или `[unset]`, так что сборку можно проверить или повторно запустить без наличия родительских / фрагментных файлов. Хэш `build_id` также отражает развёрнутую форму, поэтому изменение только в родителе корректно инвалидирует кэш.

## Example

Postgres + Redis с паролем, извлекаемым из env:

```toml
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast" }
auto_create_db = true

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["/var/coast-data/redis:/data"]

[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"
services = ["postgres"]
```

## See Also

- [Shared Service Groups](../shared_service_groups/README.md) -- обзор концепции
- [SSG Building](../shared_service_groups/BUILDING.md) -- что `coast ssg build` делает с этим файлом
- [SSG Volumes](../shared_service_groups/VOLUMES.md) -- формы объявления томов, права доступа и рецепт миграции host-volume
- [SSG Secrets](../shared_service_groups/SECRETS.md) -- конвейер extract во время сборки / inject во время запуска для `[secrets.*]`
- [SSG Routing](../shared_service_groups/ROUTING.md) -- канонические / динамические / виртуальные порты
- [Coastfile: Shared Services](SHARED_SERVICES.md) -- синтаксис `from_group = true` на стороне потребителя
- [Coastfile: Secrets and Injection](SECRETS.md) -- справочник по обычному `[secrets.*]` в Coastfile
- [Coastfile Inheritance](INHERITANCE.md) -- общая ментальная модель `extends` / `includes` / `[unset]`
