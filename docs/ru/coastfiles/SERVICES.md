# Bare Services

> **Примечание:** Bare-сервисы запускаются напрямую внутри контейнера Coast как обычные процессы — они не контейнеризируются. Если ваши сервисы уже упакованы в Docker, используйте вместо этого `compose`. Bare-сервисы лучше всего подходят для простых конфигураций, где вы хотите избежать накладных расходов на написание Dockerfile и docker-compose.yml.

Разделы `[services.*]` определяют процессы, которые Coast запускает напрямую внутри контейнера DinD, без Docker Compose. Это альтернатива использованию файла `compose` — вы не можете использовать оба варианта в одном Coastfile.

Bare-сервисы контролируются (supervised) Coast с перехватом логов и необязательными политиками перезапуска. Для более глубокого понимания того, как работают bare-сервисы, их ограничений и того, когда следует мигрировать на compose, см. [Bare Services](../concepts_and_terminology/BARE_SERVICES.md).

## Определение сервиса

Каждый сервис — это именованный раздел TOML внутри `[services]`. Поле `command` является обязательным.

```toml
[services.web]
command = "node server.js"
port = 3000
```

### `command` (обязательно)

Команда оболочки, которую нужно выполнить. Не должна быть пустой или состоять только из пробельных символов.

```toml
[services.web]
command = "npx next dev --turbopack --port 3000 --hostname 0.0.0.0"
```

### `port`

Порт, на котором сервис слушает. Используется для проверки работоспособности (health checking) и интеграции проброса портов. Если указан, должен быть ненулевым.

```toml
[services.web]
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

### `restart`

Политика перезапуска, если процесс завершился. По умолчанию `"no"`.

- `"no"` — не перезапускать
- `"on-failure"` — перезапускать только если процесс завершился с ненулевым кодом
- `"always"` — всегда перезапускать

```toml
[services.web]
command = "node server.js"
port = 3000
restart = "on-failure"
```

### `install`

Команды, которые нужно выполнить перед запуском сервиса (например, установка зависимостей). Принимает либо одну строку, либо массив строк.

```toml
[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

```toml
[services.web]
install = ["npm install", "npm run build"]
command = "npm start"
port = 3000
```

## Взаимоисключение с compose

Coastfile не может определять одновременно `compose` и `[services]`. Если у вас есть поле `compose` в `[coast]`, добавление любого раздела `[services.*]` является ошибкой. Выберите один подход для каждого Coastfile.

Если вам нужно, чтобы часть сервисов была контейнеризована через compose, а часть запускалась как bare, используйте compose для всех — см. [рекомендации по миграции в Bare Services](../concepts_and_terminology/BARE_SERVICES.md) о том, как перейти от bare-сервисов к compose.

## Примеры

### Односервисное приложение Next.js

```toml
[coast]
name = "my-frontend"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --turbopack --port 3002 --hostname 0.0.0.0"
port = 3002
restart = "on-failure"

[ports]
web = 3002
```

### Веб-сервер с фоновым воркером

```toml
[coast]
name = "my-app"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "node server.js"
port = 3000
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
web = 3000
```

### Python-сервис с многошаговой установкой

```toml
[coast]
name = "ml-service"

[coast.setup]
packages = ["python3", "py3-pip"]

[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
restart = "on-failure"

[ports]
api = 8000
```
