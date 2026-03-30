# Типы Coastfile

Один проект может иметь несколько Coastfile для разных сценариев использования. Каждый вариант называется «типом». Типы позволяют компоновать конфигурации, которые имеют общую базу, но различаются тем, какие сервисы запускаются, как обрабатываются тома или запускаются ли сервисы автоматически.

## Как работают типы

Соглашение об именовании: `Coastfile` для типа по умолчанию и `Coastfile.{type}` для вариантов. Суффикс после точки становится именем типа:

- `Coastfile` -- тип по умолчанию
- `Coastfile.test` -- тестовый тип
- `Coastfile.snap` -- тип снимка
- `Coastfile.light` -- облегчённый тип

Любой Coastfile может иметь необязательное расширение `.toml` для подсветки синтаксиса в редакторе. Суффикс `.toml` удаляется перед определением типа, поэтому эти пары эквивалентны:

- `Coastfile.toml` = `Coastfile` (тип по умолчанию)
- `Coastfile.test.toml` = `Coastfile.test` (тестовый тип)
- `Coastfile.light.toml` = `Coastfile.light` (облегчённый тип)

**Правило разрешения конфликтов:** если существуют обе формы (например, `Coastfile` и `Coastfile.toml`, или `Coastfile.light` и `Coastfile.light.toml`), вариант `.toml` имеет приоритет.

**Зарезервированные имена типов:** `"default"` и `"toml"` нельзя использовать в качестве имён типов. `Coastfile.default` и `Coastfile.toml` (как суффикс типа, то есть файл, буквально названный `Coastfile.toml.toml`) отклоняются.

Вы собираете и запускаете типизированные Coast с помощью `--type`:

```bash
coast build --type test
coast run test-1 --type test
coast exec test-1 -- go test ./...
```

## extends

Типизированный Coastfile наследуется от родительского через `extends`. Всё из родительского файла объединяется. Дочернему файлу нужно указать только то, что он переопределяет или добавляет.

```toml
[coast]
extends = "Coastfile"
```

Это позволяет избежать дублирования всей конфигурации для каждого варианта. Дочерний файл наследует все [ports](PORTS.md), [secrets](SECRETS.md), [volumes](VOLUMES.md), [shared services](SHARED_SERVICES.md), [assign strategies](ASSIGN.md), команды настройки и конфигурации [MCP](MCP_SERVERS.md) от родительского. Всё, что определено в дочернем файле, имеет приоритет над родительским.

## [unset]

Удаляет определённые элементы, унаследованные от родительского файла, по имени. Можно удалять `ports`, `shared_services`, `secrets` и `volumes`.

```toml
[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]
```

Так тестовый вариант убирает общие сервисы (чтобы базы данных запускались внутри Coast с изолированными томами) и удаляет порты, которые ему не нужны.

## [omit]

Полностью удаляет compose-сервисы из сборки. Пропущенные сервисы удаляются из compose-файла и вообще не запускаются внутри Coast.

```toml
[omit]
services = ["redis", "backend", "mailhog", "web"]
```

Используйте это, чтобы исключить сервисы, не относящиеся к назначению варианта. Тестовый вариант может оставить только базу данных, миграции и тестовый раннер.

## autostart

Управляет тем, выполняется ли `docker compose up` автоматически при запуске Coast. Значение по умолчанию — `true`.

```toml
[coast]
extends = "Coastfile"
autostart = false
```

Установите `autostart = false` для вариантов, где вы хотите вручную запускать определённые команды вместо поднятия всего стека. Это часто используется для тестовых раннеров -- вы создаёте Coast, а затем используете [`coast exec`](EXEC_AND_DOCKER.md) для запуска отдельных наборов тестов.

## Распространённые шаблоны

### Тестовый вариант

`Coastfile.test`, который оставляет только то, что нужно для запуска тестов:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]

[omit]
services = ["redis", "backend", "mailhog", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[assign]
default = "none"
[assign.services]
test-runner = "rebuild"
migrations = "rebuild"
```

Каждый тестовый Coast получает собственную чистую базу данных. Порты не публикуются, потому что тесты взаимодействуют с сервисами через внутреннюю compose-сеть. `autostart = false` означает, что вы запускаете тесты вручную через `coast exec`.

### Вариант снимка

`Coastfile.snap`, который инициализирует каждый Coast копией существующих томов баз данных на хосте:

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis"]

[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "my_project_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "my_project_redis_data"
service = "redis"
mount = "/data"
```

Общие сервисы удаляются, чтобы базы данных запускались внутри каждого Coast. `snapshot_source` инициализирует изолированные тома из существующих томов хоста во время сборки. После создания данные каждого экземпляра расходятся независимо.

### Облегчённый вариант

`Coastfile.light`, который сводит проект к минимуму для конкретного рабочего процесса -- возможно, только backend-сервис и его база данных для быстрой итерации.

## Независимые пулы сборок

Каждый тип имеет собственную символьную ссылку `latest-{type}` и собственный пул автоочистки из 5 сборок:

```bash
coast build              # обновляет latest, очищает сборки default
coast build --type test  # обновляет latest-test, очищает сборки test
coast build --type snap  # обновляет latest-snap, очищает сборки snap
```

Сборка типа `test` не влияет на сборки `default` или `snap`. Очистка полностью независима для каждого типа.

## Запуск типизированных Coast

Экземпляры, созданные с `--type`, помечаются своим типом. Для одного и того же проекта можно одновременно запускать экземпляры разных типов:

```bash
coast run dev-1                    # тип по умолчанию
coast run test-1 --type test       # тестовый тип
coast run snapshot-1 --type snap   # тип снимка

coast ls
# Все три отображаются, каждый со своим типом, портами и стратегией томов
```

Так можно одновременно держать запущенными полную dev-среду, изолированные тестовые раннеры и экземпляры, инициализированные из снимков, — всё для одного и того же проекта, всё в одно и то же время.
