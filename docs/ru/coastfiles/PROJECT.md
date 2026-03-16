# Проект и настройка

Раздел `[coast]` — единственный обязательный раздел в Coastfile. Он идентифицирует проект и настраивает, как создаётся контейнер Coast. Необязательный подраздел `[coast.setup]` позволяет устанавливать пакеты и выполнять команды внутри контейнера во время сборки.

## `[coast]`

### `name` (обязательно)

Уникальный идентификатор проекта. Используется в именах контейнеров, именах томов, отслеживании состояния и выводе CLI.

```toml
[coast]
name = "my-app"
```

### `compose`

Путь к файлу Docker Compose. Относительные пути разрешаются относительно корня проекта (директории, содержащей Coastfile, или `root`, если задан).

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
```

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
```

Если опущено, контейнер Coast запускается без выполнения `docker compose up`. Вы можете либо использовать [bare services](SERVICES.md), либо взаимодействовать с контейнером напрямую через `coast exec`.

Нельзя задавать одновременно `compose` и `[services]` в одном Coastfile.

### `runtime`

Какую среду выполнения контейнеров использовать. По умолчанию `"dind"` (Docker-in-Docker).

- `"dind"` — Docker-in-Docker с `--privileged`. Единственная среда выполнения, протестированная для продакшена. См. [Runtimes and Services](../concepts_and_terminology/RUNTIMES_AND_SERVICES.md).
- `"sysbox"` — Использует runtime Sysbox вместо привилегированного режима. Требует установленного Sysbox.
- `"podman"` — Использует Podman как внутреннюю среду выполнения контейнеров.

```toml
[coast]
name = "my-app"
runtime = "dind"
```

### `root`

Переопределяет корневую директорию проекта. По умолчанию корень проекта — это директория, содержащая Coastfile. Относительный путь разрешается относительно директории Coastfile; абсолютный путь используется как есть.

```toml
[coast]
name = "my-app"
root = "../my-project"
```

Это встречается редко. Большинство проектов держат Coastfile в фактическом корне проекта.

### `worktree_dir`

Директории, в которых находятся git worktrees. Принимает одну строку или массив строк. По умолчанию `".worktrees"`.

```toml
# Single directory
worktree_dir = ".worktrees"

# Multiple directories, including an external one
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

Относительные пути разрешаются относительно корня проекта. Пути, начинающиеся с `~/` или `/`, считаются **внешними** директориями — Coast добавляет отдельный bind mount, чтобы контейнер мог получить к ним доступ. Так интегрируются инструменты вроде Codex, которые создают worktrees вне корня проекта.

Во время выполнения Coast автоматически определяет директорию worktree по существующим git worktrees (через `git worktree list`) и предпочитает её настроенному значению по умолчанию, когда все worktrees указывают на одну директорию.

Полное описание, включая поведение внешних директорий, фильтрацию по проекту и примеры, см. в [Worktree Directories](WORKTREE_DIR.md).

### `default_worktree_dir`

Какую директорию использовать при создании **новых** worktrees. По умолчанию это первая запись в `worktree_dir`. Имеет значение только тогда, когда `worktree_dir` — массив.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
default_worktree_dir = ".worktrees"
```

### `autostart`

Нужно ли автоматически выполнять `docker compose up` (или запускать bare services) при создании экземпляра Coast с помощью `coast run`. По умолчанию `true`.

Установите `false`, когда хотите, чтобы контейнер работал, но сервисы запускались вручную — это полезно для вариантов test-runner, где вы запускаете тесты по требованию.

```toml
[coast]
name = "my-app"
extends = "Coastfile"
autostart = false
```

### `primary_port`

Задаёт порт из раздела `[ports]`, который использовать для быстрых ссылок и маршрутизации по поддоменам. Значение должно совпадать с ключом, определённым в `[ports]`.

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

См. [Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md) о том, как это включает маршрутизацию по поддоменам и шаблоны URL.

## `[coast.setup]`

Настраивает сам контейнер Coast — устанавливает инструменты, выполняет шаги сборки и материализует конфигурационные файлы. Всё в `[coast.setup]` выполняется внутри контейнера DinD (не внутри ваших compose-сервисов).

### `packages`

Пакеты APK для установки. Это пакеты Alpine Linux, поскольку базовый образ DinD основан на Alpine.

```toml
[coast.setup]
packages = ["nodejs", "npm", "git", "curl"]
```

### `run`

Команды оболочки, выполняемые по порядку во время сборки. Используйте их для установки инструментов, которые недоступны в виде APK-пакетов.

```toml
[coast.setup]
packages = ["nodejs", "npm", "python3", "wget", "bash", "ca-certificates"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]
```

### `[[coast.setup.files]]`

Файлы, которые нужно создать внутри контейнера. Каждая запись содержит `path` (обязательно, должен быть абсолютным), `content` (обязательно) и необязательный `mode` (восьмеричная строка из 3–4 цифр).

```toml
[coast.setup]
packages = ["nodejs", "npm"]
run = ["mkdir -p /app/config"]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```

Правила валидации для записей файлов:

- `path` должен быть абсолютным (начинаться с `/`)
- `path` не должен содержать компоненты `..`
- `path` не должен оканчиваться на `/`
- `mode` должен быть восьмеричной строкой из 3 или 4 цифр (например, `"600"`, `"0644"`)

## Полный пример

Контейнер Coast, настроенный для разработки на Go и Node.js:

```toml
[coast]
name = "my-fullstack-app"
compose = "./docker-compose.yml"
runtime = "dind"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
primary_port = "web"

[coast.setup]
packages = ["nodejs", "npm", "python3", "make", "curl", "git", "bash", "ca-certificates", "wget", "gcc", "musl-dev"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz && ln -s /usr/local/go/bin/go /usr/local/bin/go",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
    "pip3 install --break-system-packages pgcli",
]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```
