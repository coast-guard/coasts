# CLI и конфигурация

На этой странице рассматриваются группа команд `coast remote`, формат конфигурации `Coastfile.remote` и управление диском для удалённых машин.

## Команды управления удалёнными машинами

### `coast remote add`

Зарегистрировать удалённую машину в демоне:

```bash
coast remote add <name> <user>@<host> [--key <path>]
coast remote add <name> <user>@<host>:<port> [--key <path>]
```

Примеры:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
coast remote add dev-box ec2-user@10.50.56.218:22 --key ~/.ssh/coast_key
```

Данные подключения хранятся в `state.db` демона. Они никогда не сохраняются в Coastfile.

### `coast remote ls`

Вывести список всех зарегистрированных удалённых машин:

```bash
coast remote ls
```

### `coast remote rm`

Удалить зарегистрированную удалённую машину:

```bash
coast remote rm <name>
```

Если на удалённой машине всё ещё запущены инстансы, сначала удалите их с помощью `coast rm`.

### `coast remote test`

Проверить SSH-подключение и доступность coast-service:

```bash
coast remote test <name>
```

Эта команда проверяет доступ по SSH, подтверждает, что coast-service доступен на порту 31420 через SSH-туннель, и сообщает архитектуру удалённой машины и версию coast-service.

### `coast remote prune`

Очистить осиротевшие ресурсы на удалённой машине:

```bash
coast remote prune <name>              # remove orphaned resources
coast remote prune <name> --dry-run    # preview what would be removed
```

Prune определяет осиротевшие ресурсы, сопоставляя Docker volumes и каталоги рабочих пространств с базой данных инстансов coast-service. Ресурсы, принадлежащие активным инстансам, никогда не удаляются.

## Конфигурация Coastfile

Удалённые coasts используют отдельный Coastfile, который расширяет вашу базовую конфигурацию. Имя файла определяет тип:

| File | Type |
|------|------|
| `Coastfile.remote` | `remote` |
| `Coastfile.remote.toml` | `remote` |
| `Coastfile.remote.light` | `remote.light` |
| `Coastfile.remote.light.toml` | `remote.light` |

### Минимальный пример

```toml
[coast]
name = "my-app"
extends = "Coastfile"

[remote]
workspace_sync = "mutagen"
```

### Раздел `[remote]`

Раздел `[remote]` задаёт параметры синхронизации. Данные подключения (host, user, SSH key) берутся из `coast remote add` и определяются во время выполнения.

| Field | Default | Description |
|-------|---------|-------------|
| `workspace_sync` | `"rsync"` | Стратегия синхронизации: `"rsync"` для только однократной массовой передачи, `"mutagen"` для rsync + непрерывной синхронизации в реальном времени |

### Ограничения валидации

1. Раздел `[remote]` обязателен, когда тип Coastfile начинается с `remote`.
2. Неудалённые Coastfile не могут содержать раздел `[remote]`.
3. Встроенная конфигурация host не поддерживается. Данные подключения должны поступать из зарегистрированной удалённой машины.
4. Общие volumes с `strategy = "shared"` создают Docker volume на удалённом хосте, общий для всех coasts на этом удалённом хосте. Этот volume не распределяется между разными удалёнными машинами.

### Наследование

Удалённые Coastfile используют ту же [систему наследования](../coastfiles/INHERITANCE.md), что и другие типизированные Coastfile. Директива `extends = "Coastfile"` объединяет базовую конфигурацию с удалёнными переопределениями. Вы можете переопределять порты, сервисы, volumes и назначать стратегии так же, как и в любом другом типизированном варианте.

## Управление диском

### Использование ресурсов на один инстанс

Каждый инстанс удалённого coast потребляет приблизительно:

| Resource | Size | Location |
|----------|------|----------|
| DinD Docker volume | 3-5 GB | Remote Docker storage |
| Workspace directory | 50-300 MB | `/data/workspaces/{project}/{instance}` |
| Image tarballs | 2-3 GB | `/data/image-cache/*.tar` (shared across instances) |
| Build artifacts | 200-500 MB | `/data/images/{project}/{build_id}/` |

Рекомендуемый минимальный объём диска: **50 GB** для типичных проектов с 2–3 одновременными инстансами.

### Соглашения об именовании ресурсов

| Resource | Naming pattern |
|----------|---------------|
| DinD volume | `coast-dind--{project}--{instance}` |
| Workspace | `/data/workspaces/{project}/{instance}` |
| Image cache | `/data/image-cache/*.tar` |
| Build artifacts | `/data/images/{project}/{build_id}/` |

### Очистка при `coast rm`

Когда `coast rm` удаляет удалённый инстанс, он очищает:

1. Удалённый контейнер DinD (через coast-service)
2. Docker volume DinD (`coast-dind--{project}--{name}`)
3. Каталог рабочего пространства (`/data/workspaces/{project}/{name}`)
4. Локальную запись теневого инстанса, выделения портов и shell-контейнер

### Когда выполнять prune

Если `df -h` на удалённой машине показывает высокое использование диска после удаления инстансов, возможно, после неудачных или прерванных операций остались осиротевшие ресурсы. Запустите `coast remote prune`, чтобы освободить место:

```bash
# See what would be removed
coast remote prune my-vm --dry-run

# Actually remove
coast remote prune my-vm
```

Prune никогда не удаляет ресурсы, принадлежащие активным инстансам.
