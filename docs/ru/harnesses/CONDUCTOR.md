# Conductor

[Conductor](https://conductor.build/) запускает параллельных агентов Claude Code, каждый в своей изолированной рабочей области. Рабочие области — это git worktree, хранящиеся в `~/conductor/workspaces/<project-name>/`. Каждая рабочая область checkout'ится на именованной ветке.

Поскольку эти worktree находятся вне корня проекта, Coast требуется явная конфигурация, чтобы обнаруживать и монтировать их.

## Настройка

Добавьте `~/conductor/workspaces/<project-name>` в `worktree_dir`. В отличие от Codex (который хранит все проекты в одном плоском каталоге), Conductor размещает worktree во вложенном подкаталоге для каждого проекта, поэтому путь должен включать имя проекта:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/conductor/workspaces/my-app"]
```

Conductor позволяет настраивать путь к рабочим областям для каждого репозитория, поэтому путь по умолчанию `~/conductor/workspaces` может не соответствовать вашей настройке. Проверьте настройки вашего репозитория Conductor, чтобы найти фактический путь, и скорректируйте его соответствующим образом — принцип одинаков независимо от того, где находится каталог.

Coast разворачивает `~` во время выполнения и считает любой путь, начинающийся с `~/` или `/`, внешним. Подробности см. в [Worktree Directories](../coastfiles/WORKTREE_DIR.md).

После изменения `worktree_dir` существующие инстансы нужно **пересоздать**, чтобы bind mount вступил в силу:

```bash
coast rm my-instance
coast build
coast run my-instance
```

Список worktree обновляется сразу (Coast читает новый Coastfile), но назначение на worktree Conductor требует bind mount внутри контейнера.

## Что делает Coast

- **Bind mount** — При создании контейнера Coast монтирует `~/conductor/workspaces/<project-name>` в контейнер по пути `/host-external-wt/{index}`.
- **Обнаружение** — `git worktree list --porcelain` ограничен репозиторием, поэтому отображаются только worktree, принадлежащие текущему проекту.
- **Именование** — Worktree Conductor используют именованные ветки, поэтому они отображаются по имени ветки в UI и CLI Coast (например, `scroll-to-bottom-btn`). Ветка может быть checkout'нута только в одной рабочей области Conductor одновременно.
- **Назначение** — `coast assign` перемонтирует `/workspace` из пути внешнего bind mount.
- **Синхронизация gitignored** — Выполняется в файловой системе хоста с абсолютными путями, работает без bind mount.
- **Обнаружение orphan** — Git watcher рекурсивно сканирует внешние каталоги, фильтруя по указателям `.git` gitdir. Если Conductor архивирует или удаляет рабочую область, Coast автоматически снимает назначение с инстанса.

## Пример

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/conductor/workspaces/my-app"]
primary_port = "web"

[ports]
web = 3000
api = 8080

[assign]
default = "none"
[assign.services]
web = "hot"
api = "hot"
```

- `.worktrees/` — worktree, управляемые Coast
- `.claude/worktrees/` — Claude Code (локально, без специальной обработки)
- `~/.codex/worktrees/` — Codex (внешний, с bind mount)
- `~/conductor/workspaces/my-app/` — Conductor (внешний, с bind mount)

## Переменные окружения Conductor

- Избегайте зависимости от специфичных для Conductor переменных окружения (например, `CONDUCTOR_PORT`, `CONDUCTOR_WORKSPACE_PATH`) для конфигурации времени выполнения внутри Coast. Coast независимо управляет портами, путями рабочих областей и обнаружением сервисов — используйте вместо этого `[ports]` в Coastfile и `coast exec`.
