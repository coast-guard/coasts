# Начало работы с Coasts

Если вы ещё этого не сделали, сначала выполните установку и проверьте требования ниже. Затем это руководство покажет, как использовать Coast в проекте.

## Installing

- `brew install coast`
- `coast daemon install`

*Если вы решите не запускать `coast daemon install`, вы несёте ответственность за ручной запуск демона командой `coast daemon start` каждый раз.*

## Requirements

- macOS
- Docker Desktop
- Проект, использующий Git
- Node.js
- `socat` *(устанавливается вместе с `brew install coast` как Homebrew-зависимость `depends_on`)*

```text
Linux note: We have not tested Coasts on Linux yet, but Linux support is planned.
You can try to run Coasts on Linux today, but we do not provide guarantees that it will work correctly.
```

## Setting Up Coasts in a Project

Добавьте Coastfile в корень вашего проекта. Убедитесь, что вы не находитесь в worktree во время установки.

```text
my-project/
├── Coastfile              <-- this is what Coast reads
├── docker-compose.yml
├── Dockerfile
├── src/
│   └── ...
└── ...
```

Файл `Coastfile` указывает на ваши существующие локальные ресурсы разработки и добавляет конфигурацию, специфичную для Coasts — полный формат смотрите в [документации Coastfiles](coastfiles/README.md):

```toml
[coast]
name = "my-project"
compose = "./docker-compose.yml"

[ports]
web = 3000
db = 5432
```

Coastfile — это лёгкий TOML-файл, который *обычно* указывает на ваш существующий `docker-compose.yml` (он также работает и с не контейнеризованными локальными dev-настройками) и описывает изменения, необходимые для параллельного запуска вашего проекта — сопоставления портов, стратегии томов и секреты. Разместите его в корне проекта.

Самый быстрый способ создать Coastfile для вашего проекта — поручить это вашему агенту для написания кода.

CLI Coasts поставляется со встроенным промптом, который обучает любой AI-агент полной схеме Coastfile и CLI. Посмотреть его можно здесь: [installation_prompt.txt](installation_prompt.txt)

Передайте его напрямую вашему агенту или скопируйте [installation prompt](installation_prompt.txt) и вставьте в чат вашего агента:

```bash-emphasis
# Claude Code
claude -p "$(coast installation-prompt)"

# Codex
codex "$(coast installation-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast installation-prompt)"
```

Промпт покрывает TOML-формат Coastfile, стратегии томов, внедрение секретов и все релевантные команды CLI. Ваш агент проанализирует проект и сгенерирует Coastfile.

## Your First Coast

Перед запуском первого Coast остановите любую уже запущенную среду разработки. Если вы используете Docker Compose, выполните `docker-compose down`. Если у вас запущены локальные dev-серверы — остановите их. Coasts управляют собственными портами и будут конфликтовать со всем, что уже слушает порты.

Когда ваш Coastfile готов:

```bash
coast build
coast run dev-1
```

Проверьте, что ваш инстанс запущен:

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -             ~/dev/my-project
```

Посмотрите, где слушают ваши сервисы:

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

Каждый инстанс получает свой набор динамических портов, поэтому несколько инстансов могут работать параллельно. Чтобы сопоставить инстанс с каноническими портами вашего проекта, сделайте checkout:

```bash
coast checkout dev-1
```

Это означает, что runtime теперь «чекаутнут», и канонические порты вашего проекта (например, `3000`, `5432`) будут маршрутизироваться в этот Coast-инстанс.

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -         ✓   ~/dev/my-project
```

Чтобы открыть UI наблюдаемости Coastguard для вашего проекта:

```bash
coast ui
```

## What's Next?

- Настройте [skill для вашего host-агента](SKILLS_FOR_HOST_AGENTS.md), чтобы он знал, как взаимодействовать с Coasts
