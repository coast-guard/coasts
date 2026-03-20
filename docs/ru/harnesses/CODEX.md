# Codex

[Codex](https://developers.openai.com/codex/app/worktrees/) создаёт worktree в `$CODEX_HOME/worktrees` (обычно `~/.codex/worktrees`). Каждый worktree находится в каталоге с непрозрачным хешем, например `~/.codex/worktrees/a0db/project-name`, начинается с detached HEAD и автоматически очищается в соответствии с политикой хранения Codex.

Из [документации Codex](https://developers.openai.com/codex/app/worktrees/):

> Могу ли я управлять тем, где создаются worktree?
> Пока нет. Codex создаёт worktree в `$CODEX_HOME/worktrees`, чтобы иметь возможность единообразно управлять ими.

Поскольку эти worktree находятся вне корня проекта, Coasts требуется явная
конфигурация, чтобы обнаруживать и монтировать их.

## Setup

Добавьте `~/.codex/worktrees` в `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

Coasts разворачивает `~` во время выполнения и рассматривает любой путь,
начинающийся с `~/` или `/`, как внешний. Подробности см. в [Worktree
Directories](../coastfiles/WORKTREE_DIR.md).

После изменения `worktree_dir` существующие инстансы необходимо **пересоздать**, чтобы bind mount вступил в силу:

```bash
coast rm my-instance
coast build
coast run my-instance
```

Список worktree обновляется сразу (Coasts читает новый Coastfile), но
назначение на worktree Codex требует bind mount внутри контейнера.

## Where Coasts guidance goes

Для работы с Coasts используйте файл инструкций проекта Codex и общую
структуру skills:

- поместите краткие правила Coast Runtime в `AGENTS.md`
- поместите переиспользуемый workflow `/coasts` в `.agents/skills/coasts/SKILL.md`
- Codex показывает этот skill как команду `/coasts`
- если вы используете метаданные, специфичные для Codex, храните их рядом со
  skill в `.agents/skills/coasts/agents/openai.yaml`
- не создавайте отдельный файл команд проекта только ради документации о
  Coasts; skill — это переиспользуемая поверхность
- если этот репозиторий также использует Cursor или Claude Code, храните
  канонический skill в `.agents/skills/` и публикуйте его оттуда. См.
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) и
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md).

Например, минимальный `.agents/skills/coasts/agents/openai.yaml` может
выглядеть так:

```yaml
interface:
  display_name: "Coasts"
  short_description: "Inspect, assign, and open Coasts for this repo"
  default_prompt: "Use this skill when the user wants help finding, assigning, or opening a Coast."

policy:
  allow_implicit_invocation: false
```

Это делает skill видимым в Codex с более удобной меткой и превращает `/coasts`
в явную команду. Добавляйте `dependencies.tools` только если skill также нужны
MCP-серверы или другая настройка инструментов под управлением OpenAI.

## What Coasts does

- **Run** -- `coast run <name>` создаёт новый инстанс Coast из последней сборки. Используйте `coast run <name> -w <worktree>`, чтобы за один шаг создать и назначить worktree Codex. См. [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** -- При создании контейнера Coasts монтирует
  `~/.codex/worktrees` в контейнер по пути `/host-external-wt/{index}`.
- **Discovery** -- `git worktree list --porcelain` привязан к репозиторию, поэтому отображаются только worktree Codex, принадлежащие текущему проекту, даже если каталог содержит worktree для множества проектов.
- **Naming** -- Worktree с detached HEAD отображаются как их относительный путь внутри внешнего каталога (`a0db/my-app`, `eca7/my-app`). Worktree, основанные на ветках, отображаются по имени ветки.
- **Assign** -- `coast assign` повторно монтирует `/workspace` из пути внешнего bind mount.
- **Gitignored sync** -- Выполняется в файловой системе хоста с абсолютными путями, работает без bind mount.
- **Orphan detection** -- Наблюдатель git рекурсивно сканирует внешние каталоги,
  фильтруя по указателям gitdir в `.git`. Если Codex удаляет
  worktree, Coasts автоматически снимает назначение с инстанса.

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
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

- `.claude/worktrees/` -- Claude Code (локально, без специальной обработки)
- `~/.codex/worktrees/` -- Codex (внешний, с bind mount)

## Limitations

- Codex может очистить worktree в любой момент. Механизм обнаружения orphan в Coasts
  корректно это обрабатывает.
