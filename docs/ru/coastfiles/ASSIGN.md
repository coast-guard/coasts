# Assign

Раздел `[assign]` управляет тем, что происходит с сервисами внутри инстанса Coast, когда вы переключаете ветки с помощью `coast assign`. Каждый сервис можно настроить с разной стратегией в зависимости от того, нужен ли ему полный пересбор, перезапуск, hot-reload или вообще ничего.

О том, как `coast assign` и `coast unassign` работают во время выполнения, см. [Assign](../concepts_and_terminology/ASSIGN.md).

## `[assign]`

### `default`

Действие по умолчанию, применяемое ко всем сервисам при переключении ветки. По умолчанию `"restart"`, если весь раздел `[assign]` опущен.

- **`"none"`** — ничего не делать. Сервис продолжает работать как есть. Подходит для баз данных и кешей, которые не зависят от кода.
- **`"hot"`** — код уже live-mounted через [filesystem](../concepts_and_terminology/FILESYSTEM.md), поэтому сервис подхватывает изменения автоматически (например, через файловый watcher или hot-reload). Перезапуск контейнера не нужен.
- **`"restart"`** — перезапустить контейнер сервиса. Используйте, когда сервис читает код при старте, но не требует полной пересборки образа.
- **`"rebuild"`** — пересобрать Docker-образ сервиса и перезапустить. Требуется, когда код запекается в образ через `COPY` или `ADD` в Dockerfile.

```toml
[assign]
default = "none"
```

### `[assign.services]`

Переопределения для конкретных сервисов. Каждый ключ — это имя compose-сервиса, а значение — одно из четырёх действий выше.

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

Это позволяет оставить базы данных и кеши нетронутыми (`"none"` через значение по умолчанию), пересобирая или перезапуская только те сервисы, которые зависят от изменившегося кода.

### `[assign.rebuild_triggers]`

Шаблоны файлов, которые принудительно запускают пересборку для конкретных сервисов, даже если их действие по умолчанию легче. Каждый ключ — имя сервиса, а значение — список путей к файлам или шаблонов.

```toml
[assign]
default = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json", "package-lock.json"]
```

### `exclude_paths`

Список путей, которые нужно исключить из синхронизации worktree во время `coast assign`. Полезно в больших монорепозиториях, где некоторые директории не относятся к сервисам, запущенным в Coast, и иначе замедляли бы операцию assign.

```toml
[assign]
default = "none"
exclude_paths = ["apps/ide", "apps/extension", "apps/ide-extension"]

[assign.services]
backend = "hot"
web = "hot"
```

## Examples

### Пересобрать app, остальное не трогать

Когда ваш сервис app запекает код в свой Docker-образ, но ваши базы данных не зависят от изменений кода:

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

### Hot-reload для frontend и backend

Когда оба сервиса используют file watchers (например, dev-сервер Next.js, Go air, nodemon) и код live-mounted:

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

### Пересборка по триггерам для конкретного сервиса

Сервис API обычно просто перезапускается, но если изменились `Dockerfile` или `package.json`, он пересобирается:

```toml
[assign]
default = "none"

[assign.services]
api = "restart"
worker = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json"]
```

### Полная пересборка для всего

Когда все сервисы запекают код в свои образы:

```toml
[assign]
default = "rebuild"
```
