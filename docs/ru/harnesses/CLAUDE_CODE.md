# Claude Code

[Claude Code](https://docs.anthropic.com/en/docs/claude-code/overview) создаёт
worktree внутри проекта в `.claude/worktrees/`. Поскольку этот каталог
находится внутри репозитория, Coasts может обнаруживать и назначать worktree
Claude Code без какого-либо внешнего bind mount.

Claude Code также является здесь harness с наиболее чётким разделением на три
слоя для Coasts:

- `CLAUDE.md` для коротких, всегда активных правил работы с Coasts
- `.claude/skills/coasts/SKILL.md` для переиспользуемого workflow `/coasts`
- `.claude/commands/coasts.md` только если вам нужен файл команды как
  дополнительная точка входа

## Настройка

Добавьте `.claude/worktrees` в `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

Поскольку `.claude/worktrees` является путём относительно проекта, внешний bind mount
не нужен.

## Куда помещать руководство по Coasts

### `CLAUDE.md`

Поместите сюда правила для Coasts, которые должны применяться в каждой задаче.
Сделайте их короткими и практическими:

- запускать `coast lookup` перед первой runtime-командой в сессии
- использовать `coast exec` для тестов, сборок и сервисных команд
- использовать `coast ps` и `coast logs` для обратной связи от runtime
- спрашивать перед созданием или переназначением Coast, если соответствия не существует

### `.claude/skills/coasts/SKILL.md`

Поместите сюда переиспользуемый workflow `/coasts`. Это правильное место для
потока, который:

1. запускает `coast lookup` и повторно использует подходящий Coast
2. переключается на `coast ls`, когда соответствия нет
3. предлагает `coast run`, `coast assign`, `coast unassign`, `coast checkout`, и
   `coast ui`
4. использует CLI Coast напрямую как контракт, а не оборачивает его

Если этот репозиторий также использует Codex, T3 Code или Cursor, см.
[Multiple Harnesses](MULTIPLE_HARNESSES.md) и храните канонический skill в
`.agents/skills/coasts/`, а затем откройте к нему доступ из Claude Code.

### `.claude/commands/coasts.md`

Claude Code также поддерживает файлы команд проекта. Для документации о Coasts
считайте это необязательным:

- используйте это только если вам действительно нужен файл команды
- один простой вариант — сделать так, чтобы команда переиспользовала тот же skill
- если вы дадите команде собственные отдельные инструкции, вы берёте на себя
  поддержку второй копии workflow

## Пример структуры

### Только Claude Code

```text
CLAUDE.md
.claude/worktrees/
.claude/skills/coasts/SKILL.md
```

Если этот репозиторий также использует Codex, T3 Code или Cursor, используйте общий шаблон из
[Multiple Harnesses](MULTIPLE_HARNESSES.md) вместо его дублирования здесь,
потому что дублирующиеся provider-specific инструкции становится всё труднее
поддерживать синхронизированными каждый раз, когда вы добавляете ещё один harness.

## Что делает Coasts

- **Запуск** — `coast run <name>` создаёт новый экземпляр Coast из последней сборки. Используйте `coast run <name> -w <worktree>`, чтобы создать и назначить worktree Claude Code за один шаг. См. [Run](../concepts_and_terminology/RUN.md).
- **Обнаружение** — Coasts читает `.claude/worktrees` как любой другой локальный
  каталог worktree.
- **Именование** — worktree Claude Code используют то же поведение именования
  локальных worktree, что и другие worktree внутри репозитория в UI и CLI Coasts.
- **Назначение** — `coast assign` может переключать `/workspace` на worktree Claude Code
  без какого-либо внешнего уровня bind-mount indirection.
- **Синхронизация gitignored** — работает нормально, потому что worktree находятся внутри
  дерева репозитория.
- **Обнаружение orphan** — если Claude Code удаляет worktree, Coasts может обнаружить
  отсутствующий gitdir и при необходимости снять назначение.

## Пример

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

- `.claude/worktrees/` — worktree Claude Code
- `~/.codex/worktrees/` — worktree Codex, если вы также используете Codex в этом репозитории

## Ограничения

- Если вы дублируете один и тот же workflow `/coasts` в `CLAUDE.md`,
  `.claude/skills` и `.claude/commands`, эти копии будут расходиться. Держите
  `CLAUDE.md` коротким, а переиспользуемый workflow — в одном skill.
- Если вы хотите, чтобы один репозиторий чисто работал в нескольких harness,
  предпочитайте общий шаблон из [Multiple Harnesses](MULTIPLE_HARNESSES.md).
