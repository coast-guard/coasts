# T3 Code

[T3 Code](https://github.com/pingdotgg/t3code) — это open-source обвязка для coding agent от Ping. Каждое рабочее пространство представляет собой git worktree, хранящийся в `~/.t3/worktrees/<project-name>/`, с checkout на именованную ветку.

Поскольку эти worktree находятся вне корня проекта, Coast требуется явная конфигурация, чтобы обнаруживать и монтировать их.

## Setup

Добавьте `~/.t3/worktrees/<project-name>` в `worktree_dir`. T3 Code вкладывает worktree в подкаталог для каждого проекта, поэтому путь должен включать имя проекта. В примере ниже `my-app` должно совпадать с фактическим именем папки в `~/.t3/worktrees/` для вашего репозитория.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.t3/worktrees/my-app"]
```

Coast разворачивает `~` во время выполнения и рассматривает любой путь, начинающийся с `~/` или `/`, как внешний. Подробности см. в [Worktree Directories](../coastfiles/WORKTREE_DIR.md).

После изменения `worktree_dir` существующие инстансы необходимо **пересоздать**, чтобы bind mount вступил в силу:

```bash
coast rm my-instance
coast build
coast run my-instance
```

Список worktree обновляется сразу (Coast считывает новый Coastfile), но назначение на worktree T3 Code требует наличия bind mount внутри контейнера.

## What Coast does

- **Bind mount** — При создании контейнера Coast монтирует `~/.t3/worktrees/<project-name>` в контейнер по пути `/host-external-wt/{index}`.
- **Discovery** — `git worktree list --porcelain` ограничен репозиторием, поэтому отображаются только worktree, принадлежащие текущему проекту.
- **Naming** — Worktree T3 Code используют именованные ветки, поэтому они отображаются в UI и CLI Coast по имени ветки.
- **Assign** — `coast assign` перемонтирует `/workspace` из внешнего пути bind mount.
- **Gitignored sync** — Выполняется в файловой системе хоста с абсолютными путями, работает без bind mount.
- **Orphan detection** — Git watcher рекурсивно сканирует внешние директории, фильтруя по указателям gitdir в `.git`. Если T3 Code удаляет рабочее пространство, Coast автоматически снимает назначение с инстанса.

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.t3/worktrees/my-app"]
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
- `.claude/worktrees/` — Claude Code (локальные, без специальной обработки)
- `~/.codex/worktrees/` — Codex (внешние, с bind mount)
- `~/.t3/worktrees/my-app/` — T3 Code (внешние, с bind mount; замените `my-app` на имя папки вашего репозитория)

## Limitations

- Coast обнаруживает и монтирует worktree T3 Code, но не создаёт и не удаляет их.
- Новые worktree, создаваемые через `coast assign`, всегда помещаются в локальный `default_worktree_dir`, а не во внешнюю директорию.
- Не полагайтесь на специфичные для T3 Code переменные окружения для конфигурации времени выполнения внутри Coast. Coast независимо управляет портами, путями рабочих пространств и обнаружением сервисов — вместо этого используйте Coastfile `[ports]` и `coast exec`.
