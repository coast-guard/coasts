# Оптимизация bare-сервисов

[Bare services](BARE_SERVICES.md) запускаются как обычные процессы внутри контейнера Coast. Без слоёв Docker и кэшей образов производительность запуска и переключения веток зависит от того, как вы структурируете команды `install`, кэширование и стратегии assign.

## Быстрые команды установки

Поле `install` выполняется перед запуском сервиса и повторно при каждом `coast assign`. Если `install` безусловно запускает `make` или `yarn install`, каждое переключение ветки оплачивает полную стоимость установки, даже если ничего не изменилось.

**Используйте условные проверки, чтобы по возможности пропускать работу:**

```toml
[services.web]
install = "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)"
command = "cd /workspace && yarn dev:web"
```

Защита `test -f` пропускает установку, если `node_modules` уже существует. При первом запуске или после промаха по кэшу выполняется полная установка. При последующих assign, когда зависимости не изменились, это завершается мгновенно.

Для скомпилированных бинарников проверяйте, существует ли выходной файл:

```toml
[services.zoekt]
install = "cd /workspace && (test -f bin/zoekt-webserver || make zoekt)"
command = "cd /workspace && ./bin/zoekt-webserver -index .sourcebot/index -rpc"
```

## Кэширование директорий между worktree

Когда Coast переключает экземпляр bare-сервиса на новый worktree, монтирование `/workspace` меняется на другую директорию. Артефакты сборки, такие как `node_modules` или скомпилированные бинарники, остаются в старом worktree. Поле `cache` указывает Coast сохранять заданные директории между переключениями:

```toml
[services.web]
install = "cd /workspace && yarn install"
command = "cd /workspace && yarn dev"
cache = ["node_modules"]

[services.api]
install = "cd /workspace && make build"
command = "cd /workspace && ./bin/api-server"
cache = ["bin"]
```

Кэшируемые директории резервируются перед перемонтированием worktree и восстанавливаются после него. Это означает, что `yarn install` выполняется инкрементально, а не с нуля, а скомпилированные бинарники переживают переключения веток.

## Изоляция директорий для каждого экземпляра с помощью private_paths

Некоторые инструменты создают в workspace директории, содержащие состояние конкретного процесса: lock-файлы, кэши сборки или PID-файлы. Когда несколько экземпляров Coast используют один и тот же workspace совместно (одна и та же ветка, без worktree), эти директории конфликтуют.

Классический пример — Next.js, который при запуске берёт блокировку в `.next/dev/lock`. Второй экземпляр Coast видит блокировку и отказывается запускаться.

`private_paths` даёт каждому экземпляру свою изолированную директорию для указанных путей:

```toml
[coast]
name = "my-app"
private_paths = ["packages/web/.next"]
```

Каждый экземпляр получает overlay-монтирование для конкретного экземпляра по этому пути. Lock-файлы, кэши сборки и состояние Turbopack полностью изолированы. Изменения кода не требуются.

Используйте `private_paths` для любой директории, где одновременная запись нескольких экземпляров в одни и те же файлы вызывает проблемы: `.next`, `.turbo`, `.parcel-cache`, PID-файлы или базы данных SQLite.

## Подключение к общим сервисам

Когда вы используете [shared services](SHARED_SERVICES.md) для баз данных или кэшей, общие контейнеры запускаются в Docker daemon хоста, а не внутри Coast. Bare-сервисы, работающие внутри Coast, не могут обращаться к ним через `localhost`.

Вместо этого используйте `host.docker.internal`:

```toml
[services.web]
command = "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

Вы также можете использовать [secrets](../coastfiles/SECRETS.md), чтобы внедрять строки подключения как переменные окружения:

```toml
[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"
```

Compose-сервисы внутри Coast не имеют этой проблемы. Coast автоматически маршрутизирует имена хостов shared services через bridge-сеть для compose-контейнеров. Это касается только bare-сервисов.

## Встроенные переменные окружения

Команды bare-сервисов наследуют переменные окружения из контейнера Coast, включая всё, что задано через файлы `.env`, secrets и inject. Но иногда нужно переопределить конкретную переменную только для одного сервиса, не изменяя общие конфигурационные файлы.

Добавьте inline-присваивания перед командой:

```toml
[services.web]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

Inline-переменные имеют приоритет над всем остальным. Это полезно для:

- Установки `AUTH_URL` на [dynamic port](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md), чтобы auth-редиректы работали на экземплярах без checked-out состояния
- Переопределения `DATABASE_URL`, чтобы он указывал на shared service через `host.docker.internal`
- Установки флагов, специфичных для сервиса, без изменения общих файлов `.env` в workspace

## Стратегии assign для bare-сервисов

Выберите правильную [assign strategy](../coastfiles/ASSIGN.md) в зависимости от того, как каждый сервис подхватывает изменения кода:

| Strategy | Когда использовать | Примеры |
|---|---|---|
| `hot` | Сервис имеет watcher файлов, который автоматически обнаруживает изменения после перемонтирования worktree | Next.js (HMR), Vite, webpack, nodemon, tsc --watch |
| `restart` | Сервис загружает код при запуске и не отслеживает изменения | Скомпилированные Go-бинарники, Rails, Java-серверы |
| `none` | Сервис не зависит от кода в workspace или использует отдельный индекс | Серверы баз данных, Redis, поисковые индексы |

```toml
[assign]
default = "none"

[assign.services]
web = "hot"
backend = "hot"
zoekt = "none"
```

Установка значения по умолчанию `none` означает, что инфраструктурные сервисы никогда не затрагиваются при переключении веток. Только сервисы, которым важны изменения кода, перезапускаются или полагаются на hot reload.

## См. также

- [Bare Services](BARE_SERVICES.md) - полное справочное руководство по bare-сервисам
- [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md) - общая настройка производительности, включая `exclude_paths` и `rebuild_triggers`
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - использование `WEB_DYNAMIC_PORT` и связанных переменных в командах
