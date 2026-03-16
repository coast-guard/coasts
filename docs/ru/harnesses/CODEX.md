# Codex

[Codex](https://developers.openai.com/codex/app/worktrees/) создаёт worktree в `$CODEX_HOME/worktrees` (обычно `~/.codex/worktrees`). Каждый worktree находится в каталоге с непрозрачным хешем, например `~/.codex/worktrees/a0db/project-name`, начинается с detached HEAD и автоматически очищается в соответствии с политикой хранения Codex.

Из [документации Codex](https://developers.openai.com/codex/app/worktrees/):

> Могу ли я управлять тем, где создаются worktree?
> Пока нет. Codex создаёт worktree в `$CODEX_HOME/worktrees`, чтобы иметь возможность единообразно управлять ими.

Поскольку эти worktree находятся вне корня проекта, Coast требуется явная конфигурация, чтобы обнаруживать и монтировать их.

## Setup

Добавьте `~/.codex/worktrees` в `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

Coast разворачивает `~` во время выполнения и рассматривает любой путь, начинающийся с `~/` или `/`, как внешний. Подробности см. в [Worktree Directories](../coastfiles/WORKTREE_DIR.md).

После изменения `worktree_dir` существующие инстансы необходимо **пересоздать**, чтобы bind mount вступил в силу:

```bash
coast rm my-instance
coast build
coast run my-instance
```

Список worktree обновляется сразу (Coast читает новый Coastfile), но назначение на worktree Codex требует bind mount внутри контейнера.

## What Coast does

- **Bind mount** -- При создании контейнера Coast монтирует `~/.codex/worktrees` в контейнер по пути `/host-external-wt/{index}`.
- **Discovery** -- `git worktree list --porcelain` привязан к репозиторию, поэтому отображаются только worktree Codex, принадлежащие текущему проекту, даже если каталог содержит worktree для множества проектов.
- **Naming** -- Worktree с detached HEAD отображаются как их относительный путь внутри внешнего каталога (`a0db/my-app`, `eca7/my-app`). Worktree, основанные на ветках, отображаются по имени ветки.
- **Assign** -- `coast assign` повторно монтирует `/workspace` из пути внешнего bind mount.
- **Gitignored sync** -- Выполняется в файловой системе хоста с абсолютными путями, работает без bind mount.
- **Orphan detection** -- Наблюдатель git рекурсивно сканирует внешние каталоги, фильтруя по указателям gitdir в `.git`. Если Codex удаляет worktree, Coast автоматически снимает назначение с инстанса.

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

- `.worktrees/` -- worktree, управляемые Coast
- `.claude/worktrees/` -- Claude Code (локально, без специальной обработки)
- `~/.codex/worktrees/` -- Codex (внешний, с bind mount)

## Limitations

- Coast обнаруживает и монтирует worktree Codex, но не создаёт и не удаляет их.
- Codex может очистить worktree в любой момент. Механизм обнаружения orphan в Coast корректно это обрабатывает.
- Новые worktree, созданные через `coast assign`, всегда помещаются в локальный `default_worktree_dir`, а не во внешний каталог.
