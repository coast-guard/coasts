# Навыки для хост-агентов

Если вы используете ИИ-агентов для написания кода (Claude Code, Codex, Conductor, Cursor и т. п.) в проекте, который использует Coasts, вашему агенту нужен навык, который обучает его взаимодействовать с runtime Coasts. Без этого агент будет редактировать файлы, но не будет знать, как запускать тесты, проверять логи или убеждаться, что его изменения работают внутри запущенного окружения.

Это руководство проводит через настройку такого навыка.

## Почему агентам это нужно

Coasts разделяют [файловую систему](concepts_and_terminology/FILESYSTEM.md) между вашей хост-машиной и контейнером Coast. Ваш агент редактирует файлы на хосте, а запущенные сервисы внутри Coast сразу видят изменения. Но агенту всё равно нужно:

1. **Определить, с каким экземпляром Coast он работает** — `coast lookup` определяет это по текущему каталогу агента.
2. **Запускать команды внутри Coast** — тесты, сборки и другие runtime-задачи выполняются внутри контейнера через `coast exec`.
3. **Читать логи и проверять статус сервисов** — `coast logs` и `coast ps` дают агенту обратную связь от runtime.

Навык ниже обучает агента всем трём пунктам.

## Навык

Добавьте следующее в существующий навык, правила или файл промпта вашего агента. Если у вашего агента уже есть инструкции по запуску тестов или взаимодействию с вашим dev-окружением, это должно быть рядом с ними — это обучает агента использовать Coasts для runtime-операций.

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

То, как вы добавляете это, зависит от вашего агента:

### Claude Code

Добавьте текст навыка в файл `CLAUDE.md` вашего проекта или создайте для него отдельный раздел.

### Codex

Добавьте текст навыка в файл `AGENTS.md` вашего проекта.

### Cursor

Создайте файл правил по пути `.cursor/rules/coast.mdc` (или `.cursor/rules/coast.md`) в корне проекта и вставьте туда текст навыка выше.

### Другие агенты

Большинство агентов поддерживают ту или иную форму промпта или файла правил на уровне проекта. Вставьте текст навыка в то, что ваш агент читает при старте сессии.

## Дополнительное чтение

- Прочитайте [документацию по Coastfiles](coastfiles/README.md), чтобы узнать полную схему конфигурации
- Изучите команды [Coast CLI](concepts_and_terminology/CLI.md) для управления экземплярами
- Ознакомьтесь с [Coastguard](concepts_and_terminology/COASTGUARD.md) — веб-интерфейсом для наблюдения и управления вашими Coasts
- Просмотрите [Concepts & Terminology](concepts_and_terminology/README.md), чтобы получить полную картину того, как работают Coasts
