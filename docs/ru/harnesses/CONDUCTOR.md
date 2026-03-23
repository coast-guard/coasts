# Conductor

## Быстрая настройка

Требуется [Coast CLI](../GETTING_STARTED.md). Скопируйте этот prompt в чат
вашего агента, чтобы настроить Coasts автоматически:

```prompt-copy
conductor_setup_prompt.txt
```

Вы также можете получить содержимое skill через CLI: `coast skills-prompt`.

> **Важно:** Conductor запускает каждую сессию в изолированном git worktree.
> Prompt настройки создаёт файлы, которые существуют только в текущем рабочем
> пространстве — закоммитьте их и влейте в основную ветку, иначе они не будут
> доступны в новых сессиях.

После настройки **полностью закройте и заново откройте Conductor**, чтобы
изменения вступили в силу. Если команда `/coasts` не появляется, снова
закройте и откройте Conductor.

## Setup

Добавьте `~/conductor/workspaces/<project-name>` в `worktree_dir`. В отличие от Codex (который хранит все проекты в одном плоском каталоге), Conductor размещает worktree во вложенном подкаталоге для каждого проекта, поэтому путь должен включать имя проекта. В примере ниже `my-app` должно совпадать с фактическим именем папки вашего репозитория в `~/conductor/workspaces/`.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/conductor/workspaces/my-app"]
```

Conductor позволяет настраивать путь к рабочим областям для каждого репозитория, поэтому путь по умолчанию `~/conductor/workspaces` может не соответствовать вашей настройке. Проверьте настройки вашего репозитория Conductor, чтобы найти фактический путь, и скорректируйте его соответствующим образом — принцип одинаков независимо от того, где находится каталог.

Coasts разворачивает `~` во время выполнения и считает любой путь, начинающийся с `~/` или `/`, внешним. Подробности см. в [Worktree Directories](../coastfiles/WORKTREE_DIR.md).

После изменения `worktree_dir` существующие инстансы нужно **пересоздать**, чтобы bind mount вступил в силу:

```bash
coast rm my-instance
coast build
coast run my-instance
```

Список worktree обновляется сразу (Coasts читает новый Coastfile), но
назначение на worktree Conductor требует bind mount внутри контейнера.

## Where Coasts guidance goes

Рассматривайте Conductor как отдельный harness для работы с Coasts:

- поместите краткие правила Coast Runtime в `CLAUDE.md`
- используйте скрипты Conductor Repository Settings для настройки или
  поведения запуска, которое действительно специфично для Conductor
- не предполагавайте здесь поведение полных project command или project skill
  из Claude Code
- если вы добавили команду, и она не появилась, полностью закройте и снова
  откройте Conductor перед следующим тестом
- если этот репозиторий также использует другие harness, см.
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) и
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md), чтобы сохранить
  общий workflow `/coasts` в одном месте

## What Coasts does

- **Запуск** — `coast run <name>` создаёт новый инстанс Coast из последней сборки. Используйте `coast run <name> -w <worktree>`, чтобы за один шаг создать и назначить worktree Conductor. См. [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** — При создании контейнера Coasts монтирует
  `~/conductor/workspaces/<project-name>` в контейнер по пути
  `/host-external-wt/{index}`.
- **Обнаружение** — `git worktree list --porcelain` ограничен репозиторием, поэтому отображаются только worktree, принадлежащие текущему проекту.
- **Именование** — Worktree Conductor используют именованные ветки, поэтому
  они отображаются по имени ветки в UI и CLI Coasts (например,
  `scroll-to-bottom-btn`). Ветка может быть checkout'нута только в одной
  рабочей области Conductor одновременно.
- **Назначение** — `coast assign` перемонтирует `/workspace` из пути внешнего bind mount.
- **Синхронизация gitignored** — Выполняется в файловой системе хоста с абсолютными путями, работает без bind mount.
- **Обнаружение orphan** — Git watcher рекурсивно сканирует внешние каталоги,
  фильтруя по указателям `.git` gitdir. Если Conductor архивирует или
  удаляет рабочую область, Coasts автоматически снимает назначение с инстанса.

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = ["~/conductor/workspaces/my-app"]
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

- `~/conductor/workspaces/my-app/` — Conductor (внешний, с bind mount; замените `my-app` на имя папки вашего репозитория)

## Conductor Env Vars

- Избегайте зависимости от специфичных для Conductor переменных окружения (например,
  `CONDUCTOR_PORT`, `CONDUCTOR_WORKSPACE_PATH`) для конфигурации времени
  выполнения внутри Coasts. Coasts независимо управляет портами, путями
  рабочих областей и обнаружением сервисов — используйте вместо этого Coastfile `[ports]` и `coast exec`.
