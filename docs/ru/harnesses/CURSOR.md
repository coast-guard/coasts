# Cursor

[Cursor](https://cursor.com/docs/agent/overview) может работать напрямую в вашей
текущей checkout-копии, а его функция Parallel Agents также может создавать git
worktree в `~/.cursor/worktrees/<project-name>/`.

Для документации о Coasts это означает, что есть два варианта настройки:

- если вы просто используете Cursor в текущей checkout-копии, специальная
  запись `worktree_dir` для Cursor не требуется
- если вы используете Cursor Parallel Agents, добавьте директорию worktree Cursor в
  `worktree_dir`, чтобы Coasts мог обнаруживать и назначать эти worktree

## Настройка

### Только текущая checkout-копия

Если Cursor просто редактирует checkout-копию, которую вы уже открыли, Coasts не нужен
никакой специальный путь к worktree для Cursor. Coasts будет рассматривать эту
checkout-копию как любой другой корень локального репозитория.

### Cursor Parallel Agents

Если вы используете Parallel Agents, добавьте `~/.cursor/worktrees/<project-name>` в
`worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.cursor/worktrees/my-app"]
```

Cursor хранит worktree каждого агента в этой директории, отдельной для проекта. Coasts
раскрывает `~` во время выполнения и рассматривает путь как внешний, поэтому существующие
инстансы нужно пересоздать, чтобы bind mount вступил в силу:

```bash
coast rm my-instance
coast build
coast run my-instance
```

Список worktree обновляется сразу после изменения Coastfile, но
назначение на worktree Cursor Parallel Agent требует внешнего bind mount
внутри контейнера.

## Куда помещать рекомендации Coasts

### `AGENTS.md` или `.cursor/rules/coast.md`

Поместите сюда короткие, всегда активные правила Coast Runtime:

- используйте `AGENTS.md`, если хотите максимально переносимые инструкции проекта
- используйте `.cursor/rules/coast.md`, если хотите правила проекта в стиле Cursor и
  поддержку UI для настроек
- не дублируйте один и тот же блок Coast Runtime в обоих местах, если только у вас нет
  для этого ясной причины

### `.cursor/skills/coasts/SKILL.md` или общий `.agents/skills/coasts/SKILL.md`

Поместите сюда переиспользуемый workflow `/coasts`:

- для репозитория только под Cursor естественным местом будет `.cursor/skills/coasts/SKILL.md`
- для репозитория с несколькими harness храните канонический skill в
  `.agents/skills/coasts/SKILL.md`; Cursor может загружать его напрямую
- skill должен содержать реальный workflow `/coasts`: `coast lookup`,
  `coast ls`, `coast run`, `coast assign`, `coast unassign`,
  `coast checkout` и `coast ui`

### `.cursor/commands/coasts.md`

Cursor также поддерживает команды проекта. Для документации о Coasts рассматривайте команды как
необязательные:

- добавляйте команду только если хотите явную точку входа `/coasts`
- один из простых вариантов — сделать так, чтобы команда переиспользовала тот же skill
- если вы дадите команде собственные отдельные инструкции, вам придется
  поддерживать вторую копию workflow

### `.cursor/worktrees.json`

Используйте `.cursor/worktrees.json` для собственной инициализации worktree в Cursor, а не для
политики Coasts:

- устанавливать зависимости
- копировать или создавать symlink для файлов `.env`
- запускать миграции базы данных или другие одноразовые шаги инициализации

Не переносите правила Coast Runtime или workflow Coast CLI в
`.cursor/worktrees.json`.

## Пример структуры

### Только Cursor

```text
AGENTS.md
.cursor/skills/coasts/SKILL.md
.cursor/commands/coasts.md        # optional
.cursor/rules/coast.md            # optional alternative to AGENTS.md
.cursor/worktrees.json            # optional, for Parallel Agents bootstrap
```

### Cursor плюс другие harness

```text
AGENTS.md
CLAUDE.md
.agents/skills/coasts/SKILL.md
.agents/skills/coasts/agents/openai.yaml
.claude/skills/coasts -> ../../.agents/skills/coasts
.cursor/commands/coasts.md        # optional
```

## Что делает Coasts

- **Запуск** — `coast run <name>` создает новый инстанс Coast из последней сборки. Используйте `coast run <name> -w <worktree>`, чтобы создать и назначить worktree Cursor за один шаг. См. [Run](../concepts_and_terminology/RUN.md).
- **Текущая checkout-копия** — Никакой специальной обработки Cursor не требуется, когда Cursor
  работает напрямую в открытом вами репозитории.
- **Bind mount** — Для Parallel Agents Coasts монтирует
  `~/.cursor/worktrees/<project-name>` в контейнер по пути
  `/host-external-wt/{index}`.
- **Обнаружение** — `git worktree list --porcelain` по-прежнему ограничен репозиторием, поэтому Coasts
  показывает только те worktree Cursor, которые принадлежат текущему проекту.
- **Именование** — Worktree Cursor Parallel Agent отображаются по именам своих веток в
  CLI и UI Coasts.
- **Назначение** — `coast assign` перемонтирует `/workspace` из внешнего пути bind
  mount, когда выбран worktree Cursor.
- **Синхронизация gitignored** — Продолжает работать в файловой системе хоста с абсолютными
  путями.
- **Обнаружение orphan** — Если Cursor очищает старые worktree, Coasts может обнаружить
  отсутствующий gitdir и при необходимости снять их назначение.

## Пример

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.cursor/worktrees/my-app"]
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

- `.claude/worktrees/` — worktree Claude Code
- `~/.codex/worktrees/` — worktree Codex
- `~/.cursor/worktrees/my-app/` — worktree Cursor Parallel Agent

## Ограничения

- Если вы не используете Cursor Parallel Agents, не добавляйте
  `~/.cursor/worktrees/<project-name>` только потому, что редактируете в
  Cursor.
- Держите правила Coast Runtime в одном всегда активном месте: `AGENTS.md` или
  `.cursor/rules/coast.md`. Дублирование в обоих местах ведет к расхождениям.
- Держите переиспользуемый workflow `/coasts` в skill. `.cursor/worktrees.json` предназначен
  для инициализации Cursor, а не для политики Coasts.
- Если один репозиторий используется совместно в Cursor, Codex, Claude Code или T3 Code, предпочитайте
  общую структуру из [Multiple Harnesses](MULTIPLE_HARNESSES.md).
