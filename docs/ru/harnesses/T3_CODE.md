# T3 Code

[T3 Code](https://github.com/pingdotgg/t3code) создаёт git worktree в
`~/.t3/worktrees/<project-name>/`, переключённые на именованные ветки.

В T3 Code поместите всегда активные правила Coast Runtime в `AGENTS.md`, а
переиспользуемый workflow `/coasts` — в `.agents/skills/coasts/SKILL.md`.

Поскольку эти worktree находятся вне корня проекта, Coasts требуется явная
конфигурация, чтобы обнаруживать и монтировать их.

## Setup

Добавьте `~/.t3/worktrees/<project-name>` в `worktree_dir`. T3 Code размещает worktree во вложенном подкаталоге для каждого проекта, поэтому путь должен включать имя проекта. В примере ниже `my-app` должно совпадать с фактическим именем папки в `~/.t3/worktrees/` для вашего репозитория.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.t3/worktrees/my-app"]
```

Coasts разворачивает `~` во время выполнения и рассматривает любой путь,
начинающийся с `~/` или `/`, как внешний. Подробности см. в [Worktree Directories](../coastfiles/WORKTREE_DIR.md).

После изменения `worktree_dir` существующие инстансы необходимо **пересоздать**, чтобы bind mount вступил в силу:

```bash
coast rm my-instance
coast build
coast run my-instance
```

Список worktree обновляется сразу (Coasts считывает новый Coastfile), но
назначение на worktree T3 Code требует bind mount внутри контейнера.

## Where Coasts guidance goes

Для T3 Code используйте такую структуру:

- поместите краткие правила Coast Runtime в `AGENTS.md`
- поместите переиспользуемый workflow `/coasts` в `.agents/skills/coasts/SKILL.md`
- не добавляйте отдельный слой команд проекта или slash-команд, специфичный
  для T3, для Coasts
- если этот репозиторий использует несколько harness, см.
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) и
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md).

## What Coasts does

- **Run** — `coast run <name>` создаёт новый инстанс Coast из последней сборки. Используйте `coast run <name> -w <worktree>`, чтобы за один шаг создать worktree T3 Code и назначить его. См. [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** — При создании контейнера Coasts монтирует
  `~/.t3/worktrees/<project-name>` в контейнер по пути
  `/host-external-wt/{index}`.
- **Discovery** — `git worktree list --porcelain` ограничен репозиторием, поэтому отображаются только worktree, принадлежащие текущему проекту.
- **Naming** — Worktree T3 Code используют именованные ветки, поэтому они отображаются в UI и CLI Coasts по имени ветки.
- **Assign** — `coast assign` перемонтирует `/workspace` из внешнего пути bind mount.
- **Gitignored sync** — Выполняется в файловой системе хоста с абсолютными путями, работает без bind mount.
- **Orphan detection** — Git watcher рекурсивно сканирует внешние директории,
  фильтруя по указателям gitdir в `.git`. Если T3 Code удаляет рабочее
  пространство, Coasts автоматически снимает назначение с инстанса.

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

- `.claude/worktrees/` — Claude Code (локальные, без специальной обработки)
- `~/.codex/worktrees/` — Codex (внешние, с bind mount)
- `~/.t3/worktrees/my-app/` — T3 Code (внешние, с bind mount; замените `my-app` именем папки вашего репозитория)

## Limitations

- Не полагайтесь на специфичные для T3 Code переменные окружения для
  конфигурации времени выполнения внутри Coasts. Coasts независимо управляет портами, путями рабочих пространств и
  обнаружением сервисов — вместо этого используйте Coastfile `[ports]` и `coast exec`.
