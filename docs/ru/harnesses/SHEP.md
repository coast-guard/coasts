# Shep

## Быстрая настройка

Требуется [Coast CLI](../GETTING_STARTED.md). Скопируйте этот промпт в чат
вашего агента, чтобы настроить Coasts автоматически:

```prompt-copy
shep_setup_prompt.txt
```

Вы также можете получить содержимое навыка из CLI: `coast skills-prompt`.

После настройки **закройте и снова откройте редактор**, чтобы новый навык и
инструкции проекта вступили в силу.

---

[Shep](https://shep-ai.github.io/cli/) создаёт worktree в `~/.shep/repos/{hash}/wt/{branch-slug}`. Хеш — это первые 16 шестнадцатеричных символов SHA-256 от абсолютного пути репозитория, поэтому он детерминирован для каждого репозитория, но непрозрачен. Все worktree для данного репозитория используют один и тот же хеш и различаются по подкаталогу `wt/{branch-slug}`.

В CLI Shep команда `shep feat show <feature-id>` выводит путь к worktree, а
`ls ~/.shep/repos` показывает каталоги хешей для каждого репозитория.

Поскольку хеш различается для каждого репозитория, Coasts использует **glob-шаблон** для обнаружения
worktree Shep без необходимости жёстко задавать хеш пользователем.

## Настройка

Добавьте `~/.shep/repos/*/wt` в `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
```

`*` соответствует каталогу хеша для каждого репозитория. Во время выполнения Coasts разворачивает glob,
находит соответствующий каталог (например, `~/.shep/repos/a21f0cda9ab9d456/wt`) и
монтирует его в контейнер через bind mount. Подробности о glob-шаблонах см. в
[Worktree Directories](../coastfiles/WORKTREE_DIR.md).

После изменения `worktree_dir` существующие инстансы необходимо **пересоздать**, чтобы bind mount вступил в силу:

```bash
coast rm my-instance
coast build
coast run my-instance
```

Список worktree обновляется сразу (Coasts читает новый Coastfile), но
назначение на worktree Shep требует bind mount внутри контейнера.

## Куда помещать рекомендации Coasts

Shep внутри использует Claude Code, поэтому следуйте соглашениям Claude Code:

- размещайте короткие правила Coast Runtime в `CLAUDE.md`
- размещайте переиспользуемый workflow `/coasts` в `.claude/skills/coasts/SKILL.md` или
  в общем `.agents/skills/coasts/SKILL.md`
- если этот репозиторий также использует другие harness, см.
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) и
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md)

## Что делает Coasts

- **Run** -- `coast run <name>` создаёт новый инстанс Coast из последней сборки. Используйте `coast run <name> -w <worktree>`, чтобы создать и назначить worktree Shep за один шаг. См. [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** -- При создании контейнера Coasts разрешает glob
  `~/.shep/repos/*/wt` и монтирует каждый совпавший каталог в контейнер по пути
  `/host-external-wt/{index}`.
- **Discovery** -- `git worktree list --porcelain` ограничен репозиторием, поэтому
  отображаются только worktree, принадлежащие текущему проекту.
- **Naming** -- Worktree Shep используют именованные ветки, поэтому они отображаются по имени ветки
  в UI и CLI Coasts (например, `feat-green-background`).
- **Assign** -- `coast assign` перемонтирует `/workspace` из пути внешнего bind mount.
- **Gitignored sync** -- Выполняется в файловой системе хоста с абсолютными путями, работает без bind mount.
- **Orphan detection** -- Git watcher рекурсивно сканирует внешние каталоги,
  фильтруя по указателям gitdir в `.git`. Если Shep удаляет
  worktree, Coasts автоматически снимает назначение с инстанса.

## Пример

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
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

- `~/.shep/repos/*/wt` -- Shep (внешний, подключается через bind mount с помощью разворачивания glob)

## Структура путей Shep

```
~/.shep/repos/
  {sha256-of-repo-path-first-16-chars}/
    wt/
      {branch-slug}/     <-- git worktree
      {branch-slug}/
```

Ключевые моменты:
- Один и тот же репозиторий = один и тот же хеш каждый раз (детерминированный, не случайный)
- Разные репозитории = разные хеши
- Разделители пути нормализуются к `/` перед хешированием
- Хеш можно узнать через `shep feat show <feature-id>` или `ls ~/.shep/repos`
