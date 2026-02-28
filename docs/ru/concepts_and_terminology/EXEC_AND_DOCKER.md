# Exec & Docker

`coast exec` переносит вас в shell внутри DinD-контейнера Coast. Ваша рабочая директория — `/workspace` — [bind-mounted корень проекта](FILESYSTEM.md), где находится ваш Coastfile. Это основной способ запускать команды, просматривать файлы или отлаживать сервисы внутри Coast с хост-машины.

`coast docker` — вспомогательная команда для прямого взаимодействия с внутренним демоном Docker.

## `coast exec`

Открыть shell внутри инстанса Coast:

```bash
coast exec dev-1
```

Это запускает сессию `sh` в `/workspace`. Контейнеры Coast основаны на Alpine, поэтому shell по умолчанию — `sh`, а не `bash`.

Также можно выполнить конкретную команду, не входя в интерактивный shell:

```bash
coast exec dev-1 ls -la
coast exec dev-1 -- npm install
coast exec dev-1 -- go test ./...
```

Всё, что указано после имени инстанса, передаётся как команда. Используйте `--`, чтобы отделить флаги, относящиеся к вашей команде, от флагов, относящихся к `coast exec`.

### Working Directory

Shell стартует в `/workspace` — это bind-mounted в контейнер корень вашего проекта на хосте. Это означает, что ваш исходный код, Coastfile и все файлы проекта находятся прямо там:

```text
/workspace $ ls
Coastfile       README.md       apps/           packages/
Coastfile.light go.work         infra/          scripts/
Coastfile.snap  go.work.sum     package-lock.json
```

Любые изменения, которые вы делаете в файлах под `/workspace`, сразу отражаются на хосте — это bind mount, а не копия.

### Interactive vs Non-Interactive

Когда stdin — это TTY (вы печатаете в терминале), `coast exec` полностью обходит daemon и запускает `docker exec -it` напрямую для полного проброса TTY. Это означает, что цвета, перемещение курсора, автодополнение по Tab и интерактивные программы работают как ожидается.

Когда stdin направлен по pipe или запускается из скрипта (CI, агентные workflows, `coast exec dev-1 -- some-command | grep foo`), запрос проходит через daemon и возвращает структурированные stdout, stderr и код завершения.

### File Permissions

Exec запускается с UID:GID вашего пользователя на хосте, поэтому файлы, созданные внутри Coast, получают корректного владельца на хосте. Никаких несовпадений прав между хостом и контейнером.

## `coast docker`

Если `coast exec` даёт вам shell в самом DinD-контейнере, то `coast docker` позволяет запускать команды Docker CLI против **внутреннего** демона Docker — того, который управляет вашими compose-сервисами.

```bash
coast docker dev-1                    # по умолчанию: docker ps
coast docker dev-1 ps                 # то же самое, что выше
coast docker dev-1 compose ps         # docker compose ps (внутренние сервисы)
coast docker dev-1 images             # список образов во внутреннем демоне
coast docker dev-1 compose logs web   # docker compose logs для сервиса
```

Каждая переданная команда автоматически получает префикс `docker`. То есть `coast docker dev-1 compose ps` запускает `docker compose ps` внутри контейнера Coast, обращаясь к внутреннему демону.

### `coast exec` vs `coast docker`

Разница в том, на что вы нацеливаетесь:

| Command | Runs as | Target |
|---|---|---|
| `coast exec dev-1 ls /workspace` | `sh -c "ls /workspace"` в DinD-контейнере | Сам контейнер Coast (файлы проекта, установленные инструменты) |
| `coast docker dev-1 ps` | `docker ps` в DinD-контейнере | Внутренний демон Docker (контейнеры compose-сервисов) |
| `coast docker dev-1 compose logs web` | `docker compose logs web` в DinD-контейнере | Логи конкретного compose-сервиса через внутренний демон |

Используйте `coast exec` для работы на уровне проекта — запуск тестов, установка зависимостей, просмотр файлов. Используйте `coast docker`, когда нужно увидеть, что делает внутренний демон Docker — статус контейнеров, образы, сети, операции compose.

## Coastguard Exec Tab

Веб-интерфейс Coastguard предоставляет постоянный интерактивный терминал, подключённый по WebSocket.

![Exec tab in Coastguard](../../assets/coastguard-exec.png)
*Вкладка Coastguard Exec, показывающая shell-сессию в /workspace внутри инстанса Coast.*

Терминал работает на xterm.js и предлагает:

- **Постоянные сессии** — терминальные сессии сохраняются при навигации по страницам и обновлениях браузера. При переподключении воспроизводится буфер прокрутки (scrollback), чтобы вы продолжили с того же места.
- **Несколько вкладок** — откройте несколько shell одновременно. Каждая вкладка — независимая сессия.
- Вкладки **[Agent shell](AGENT_SHELLS.md)** — создавайте выделенные agent shell для AI coding agents с отслеживанием статуса active/inactive.
- **Полноэкранный режим** — разверните терминал на весь экран (Escape для выхода).

Помимо exec-вкладки на уровне инстанса, Coastguard также предоставляет терминальный доступ и на других уровнях:

- **Service exec** — перейдите в отдельный сервис на вкладке Services, чтобы получить shell внутри конкретного внутреннего контейнера (это делает двойной `docker exec` — сначала в DinD-контейнер, затем в контейнер сервиса).
- **Exec для [Shared service](SHARED_SERVICES.md)** — получите shell внутри контейнера shared service на уровне хоста.
- **Host terminal** — shell на вашей хост-машине в корне проекта, вообще не заходя в Coast.

## When to Use Which

- **`coast exec`** — запуск команд на уровне проекта (npm install, go test, просмотр файлов, отладка) внутри DinD-контейнера.
- **`coast docker`** — просмотр или управление внутренним демоном Docker (статус контейнеров, образы, сети, операции compose).
- **Вкладка Coastguard Exec** — интерактивная отладка с постоянными сессиями, несколькими вкладками и поддержкой agent shell. Лучше всего, когда вы хотите держать открытыми несколько терминалов, одновременно используя остальную часть UI.
- **`coast logs`** — для чтения вывода сервисов используйте `coast logs` вместо `coast docker compose logs`. См. [Logs](LOGS.md).
- **`coast ps`** — для проверки статуса сервисов используйте `coast ps` вместо `coast docker compose ps`. См. [Runtimes and Services](RUNTIMES_AND_SERVICES.md).
