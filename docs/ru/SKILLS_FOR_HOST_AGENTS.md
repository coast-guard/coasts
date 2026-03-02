# Навыки для хост-агентов

Если вы используете AI-агентов для кодинга (Claude Code, Codex, Conductor, Cursor или аналогичные) в проекте, который использует Coasts, вашему агенту нужен навык, который научит его взаимодействовать с рантаймом Coast. Без этого агент будет редактировать файлы, но не будет знать, как запускать тесты, проверять логи или подтверждать, что его изменения работают внутри запущенной среды.

Это руководство проводит через настройку такого навыка.

## Зачем агентам это нужно

Coasts разделяют [файловую систему](concepts_and_terminology/FILESYSTEM.md) между вашей хост-машиной и контейнером Coast. Ваш агент редактирует файлы на хосте, и запущенные сервисы внутри Coast сразу видят изменения. Но агенту всё равно нужно:

1. **Определить, с каким экземпляром Coast он работает** — `coast lookup` определяет это по текущей директории агента.
2. **Запускать команды внутри Coast** — тесты, сборки и другие задачи рантайма выполняются внутри контейнера через `coast exec`.
3. **Читать логи и проверять статус сервисов** — `coast logs` и `coast ps` дают агенту обратную связь от рантайма.

Навык ниже обучает агента всем трём пунктам.

## Навык

Добавьте следующее в существующий навык, правила или файл промпта вашего агента. Если у вашего агента уже есть инструкции по запуску тестов или взаимодействию с вашей dev-средой, это должно быть рядом с ними — это учит агента использовать Coasts для операций в рантайме.

```text-copy
This project uses Coasts (containerized host) for isolated development environments.
Your code edits are automatically visible inside the running Coast — the filesystem
is shared between the host and the container.

=== ORIENTATION ===

Before running any runtime commands, discover which Coast instance matches your
current working directory:

  coast lookup

This prints the instance name, ports, URLs, and example commands. Use the instance
name from the output for all subsequent commands.

If you need deeper context on how Coasts work, read these docs:

  coast docs --path concepts_and_terminology/LOOKUP.md
  coast docs --path concepts_and_terminology/FILESYSTEM.md
  coast docs --path concepts_and_terminology/EXEC_AND_DOCKER.md
  coast docs --path concepts_and_terminology/LOGS.md

=== RUNNING COMMANDS ===

Use `coast exec` to run commands inside the Coast. The shell starts at the workspace
root (where the Coastfile is). cd to your target directory first:

  coast exec <instance> -- sh -c "cd <dir> && <command>"

Examples:

  coast exec dev-1 -- sh -c "cd src && npm test"
  coast exec dev-1 -- sh -c "cd backend && go test ./..."
  coast exec dev-1 -- sh -c "cd apps/web && npx playwright test"

=== RUNTIME FEEDBACK ===

Check service status:

  coast ps <instance>

Read service logs:

  coast logs <instance> --service <service>
  coast logs <instance> --service <service> --tail 50

=== TROUBLESHOOTING ===

If you encounter errors or unfamiliar behavior, search the Coast docs:

  coast search-docs "error message or description"

This uses semantic search — describe the problem in natural language and it will
find the relevant documentation.

=== RULES ===

- Always run `coast lookup` before your first runtime command in a session.
- Do not run services directly on the host. Use `coast exec` for all runtime tasks.
- File edits on the host are instantly visible inside the Coast. You do not need
  to copy files or rebuild after editing.
- If `coast lookup` returns no instances, the Coast may not be running. Suggest
  `coast run dev-1` or check `coast ls` for the project state.
```

## Добавление навыка вашему агенту

Самый быстрый способ — позволить агенту настроить себя самостоятельно. Запустите одну из этих команд из директории вашего проекта:

```sh
# Claude Code
claude -p "$(coast skills-prompt)"

# Codex
codex "$(coast skills-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast skills-prompt)"
```

Это даст агенту текст навыка и инструкции записать его в собственный конфигурационный файл (`CLAUDE.md`, `AGENTS.md`, `.cursor/rules/coast.md` и т. п.).

### Ручная настройка

Если вы предпочитаете добавить навык самостоятельно:

- **Claude Code:** Добавьте текст навыка в файл `CLAUDE.md` вашего проекта.
- **Codex:** Добавьте текст навыка в файл `AGENTS.md` вашего проекта.
- **Cursor:** Создайте `.cursor/rules/coast.md` в корне проекта и вставьте текст навыка.
- **Другие агенты:** Вставьте текст навыка в любой файл промпта или правил на уровне проекта, который ваш агент читает при запуске.

## Дополнительное чтение

- Прочитайте [документацию по Coastfiles](coastfiles/README.md), чтобы изучить полную схему конфигурации
- Изучите команды [Coast CLI](concepts_and_terminology/CLI.md) для управления экземплярами
- Ознакомьтесь с [Coastguard](concepts_and_terminology/COASTGUARD.md), веб-интерфейсом для наблюдения и управления вашими Coasts
- Просмотрите [Concepts & Terminology](concepts_and_terminology/README.md), чтобы получить полную картину того, как работают Coasts
