# Навыки для хост-агентов

Если вы используете AI-агентов для программирования на хосте, пока ваше приложение работает внутри Coasts, вашему агенту обычно нужны две специфичные для Coast части настройки:

1. всегда активный раздел Coast Runtime в файле проектных инструкций или файле правил вашего harness
2. переиспользуемый навык рабочего процесса Coast, такой как `/coasts`, если harness поддерживает проектные навыки

Без первой части агент редактирует файлы, но забывает использовать `coast exec`.
Без второй каждое назначение Coast, лог и UI-процесс приходится заново
объяснять в чате.

Это руководство делает настройку конкретной и специфичной для Coast: какой
файл создать, какой текст в него поместить и как это меняется в зависимости от harness.

## Why agents need this

Coasts предоставляет общий [filesystem](concepts_and_terminology/FILESYSTEM.md) между
вашей хост-машиной и контейнером Coast. Ваш агент редактирует файлы на хосте,
а запущенные внутри Coast сервисы сразу видят изменения. Но агенту всё равно
необходимо:

1. определить, какой экземпляр Coast соответствует текущему checkout
2. запускать тесты, сборки и runtime-команды внутри этого Coast
3. читать логи и статус сервисов из Coast
4. безопасно обрабатывать назначение worktree, когда Coast ещё не подключён

## What goes where

- `AGENTS.md`, `CLAUDE.md` или `.cursor/rules/coast.md` — короткие правила Coast,
  которые должны применяться в каждой задаче, даже если ни один навык не вызван
- навык (`.agents/skills/...`, `.claude/skills/...` или `.cursor/skills/...`)
  — сам переиспользуемый workflow Coast, такой как `/coasts`
- файл команды (`.claude/commands/...` или `.cursor/commands/...`) — необязательная
  явная точка входа для harness, которые это поддерживают; один из простых вариантов —
  сделать так, чтобы команда переиспользовала навык

Если один репозиторий использует больше одного harness, храните канонический
навык Coast в одном месте и публикуйте его там, где нужно. См.
[Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md).

## 1. Always-on Coast Runtime rules

Добавьте следующий блок в всегда активный файл проектных инструкций или
файл правил соответствующего harness (`AGENTS.md`, `CLAUDE.md`, `.cursor/rules/coast.md` или эквивалент):

```text-copy
# Coast Runtime

This project uses Coasts — containerized runtimes for running services, tests,
and other runtime commands. The filesystem is shared between the host and the
container, so file edits on either side are visible to both immediately.

## Discovery

Before the first runtime command in a session, run:

  coast lookup

This prints the instance name, ports, and example commands. Use the instance
name from the output for all subsequent commands.

## What runs where

The filesystem is shared, so only use `coast exec` for things that need the
container runtime (databases, services, integration tests). Everything else
runs directly on the host.

Use `coast exec` for:
- Tests that need running services (integration tests, API tests)
- Service restarts or compose operations
- Anything that talks to databases, caches, or other container services

Run directly on the host:
- Linting, typechecking, formatting
- Git operations
- Playwright and browser tests
- Installing host-side dependencies (npm install, pip install)
- File search, code generation, static analysis

Example:

  coast exec <instance> -- sh -c "cd <dir> && npm test"    # needs DB
  npm run lint                                              # host is fine
  npx playwright test                                       # host is fine

## Runtime feedback

  coast ps <instance>
  coast logs <instance> --service <service>
  coast logs <instance> --service <service> --tail 50

## Creating and assigning Coasts

If `coast lookup` returns no match, run `coast ls` to see what exists.

If an unassigned Coast is already running for this project, prefer assigning
your worktree to it rather than creating a new one:

  coast assign <existing> -w <worktree>

If no Coast is running, ask the user before creating one — Coasts can be
memory intensive:

  coast run <name> -w <worktree>

A project must be built before instances can be created. If `coast run` fails
because no build exists, run `coast build` first.

## Coastfile setup

If the project does not have a Coastfile yet, or if you need to modify the
Coastfile, read the Coastfile docs first:

  coast docs --path coastfiles/README.md

## When confused

Before guessing about Coast behavior, explore the docs:

  coast docs                                     # list all doc pages
  coast docs --path concepts_and_terminology/RUN.md
  coast docs --path concepts_and_terminology/ASSIGN.md
  coast docs --path concepts_and_terminology/BUILDS.md
  coast search-docs "your question here"         # semantic search

## Rules

- Always run `coast lookup` before your first runtime command in a session.
- Use `coast exec` only for things that need the container runtime.
- Run linting, typechecking, formatting, and git on the host directly.
- Use `coast docs` or `coast search-docs` before guessing about Coast behavior.
- Do not run services directly on the host when the project expects Coast.
```

Этот блок должен находиться в всегда активном файле, потому что эти правила
должны применяться в каждой задаче, а не только когда агент явно входит в workflow `/coasts`.

## 2. Reusable `/coasts` skill

Когда harness поддерживает проектные навыки, сохраните содержимое навыка как
`SKILL.md` в каталоге ваших навыков. Полный текст навыка находится в
[skills_prompt.txt](skills_prompt.txt) (если вы в режиме CLI, используйте
`coast skills-prompt`) — всё после блока Coast Runtime является содержимым навыка,
начиная с frontmatter `---`.

Если вы используете Codex или поверхности, специфичные для OpenAI, вы можете
при желании добавить `agents/openai.yaml` рядом с навыком для отображаемых
метаданных или политики вызова. Эти метаданные должны находиться рядом с
навыком, а не заменять его.

## Harness quick start

| Harness | Always-on file | Reusable Coast workflow | Notes |
|---------|----------------|-------------------------|-------|
| OpenAI Codex | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | Нет отдельного файла проектной команды, который можно было бы рекомендовать для документации Coast. См. [Codex](harnesses/CODEX.md). |
| Claude Code | `CLAUDE.md` | `.claude/skills/coasts/SKILL.md` | `.claude/commands/coasts.md` необязателен, но храните логику в навыке. См. [Claude Code](harnesses/CLAUDE_CODE.md). |
| Cursor | `AGENTS.md` или `.cursor/rules/coast.md` | `.cursor/skills/coasts/SKILL.md` или общий `.agents/skills/coasts/SKILL.md` | `.cursor/commands/coasts.md` необязателен. `.cursor/worktrees.json` предназначен для bootstrap worktree в Cursor, а не для политики Coast. См. [Cursor](harnesses/CURSOR.md). |
| Conductor | `CLAUDE.md` | Начните с `CLAUDE.md`; используйте скрипты и настройки Conductor для поведения, специфичного для Conductor | Не предполагайте полную поддержку проектных команд Claude Code. Если новая команда не появляется, полностью закройте и снова откройте Conductor. См. [Conductor](harnesses/CONDUCTOR.md). |
| T3 Code | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | Это самая ограниченная поверхность harness из перечисленных здесь. Используйте layout в стиле Codex и не придумывайте отдельный слой команд T3 для документации Coast. См. [T3 Code](harnesses/T3_CODE.md). |

## Let the agent set itself up

Самый быстрый способ — позволить агенту самому записать правильные файлы.
Скопируйте приведённый ниже промпт в чат вашего агента — он включает блок Coast Runtime,
блок навыка `coasts` и специфичные для harness инструкции о том, где должна
находиться каждая часть.

```prompt-copy
skills_prompt.txt
```

Вы также можете получить тот же вывод из CLI, выполнив `coast skills-prompt`.

## Manual setup

- **Codex:** поместите раздел Coast Runtime в `AGENTS.md`, затем поместите
  переиспользуемый навык `coasts` в `.agents/skills/coasts/SKILL.md`.
- **Claude Code:** поместите раздел Coast Runtime в `CLAUDE.md`, затем поместите
  переиспользуемый навык `coasts` в `.claude/skills/coasts/SKILL.md`. Добавляйте
  `.claude/commands/coasts.md` только если вам специально нужен файл команды.
- **Cursor:** поместите раздел Coast Runtime в `AGENTS.md`, если хотите наиболее
  переносимые инструкции, или в `.cursor/rules/coast.md`, если хотите
  project rule в стиле Cursor. Поместите переиспользуемый workflow `coasts` в
  `.cursor/skills/coasts/SKILL.md` для репозитория только под Cursor или в
  `.agents/skills/coasts/SKILL.md`, если репозиторий используется совместно с другими harness.
  Добавляйте `.cursor/commands/coasts.md` только если вам специально нужен явный
  файл команды.
- **Conductor:** поместите раздел Coast Runtime в `CLAUDE.md`. Используйте
  скрипты Repository Settings в Conductor для bootstrap или поведения запуска,
  специфичного для Conductor. Если вы добавили команду, а она не появляется,
  полностью закройте и снова откройте приложение.
- **T3 Code:** используйте тот же layout, что и для Codex: `AGENTS.md` плюс
  `.agents/skills/coasts/SKILL.md`. Рассматривайте T3 Code здесь как тонкий
  harness в стиле Codex, а не как отдельную поверхность команд Coast.
- **Multiple harnesses:** храните канонический навык в
  `.agents/skills/coasts/SKILL.md`. Cursor может загружать его напрямую; при необходимости
  опубликуйте его для Claude Code через `.claude/skills/coasts/`.

## Further reading

- Прочитайте [руководство по Harnesses](harnesses/README.md), чтобы увидеть матрицу по каждому harness
- Прочитайте [Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md), чтобы узнать о шаблоне общей структуры
- Прочитайте [документацию Coastfiles](coastfiles/README.md), чтобы изучить полную
  схему конфигурации
- Изучите команды [Coast CLI](concepts_and_terminology/CLI.md) для управления
  экземплярами
- Ознакомьтесь с [Coastguard](concepts_and_terminology/COASTGUARD.md), веб-интерфейсом для
  наблюдения и управления вашими Coasts
