# Начало работы с Coasts

Если вы ещё этого не сделали, сначала выполните установку и проверьте требования ниже. Затем это руководство проведёт вас через использование Coast в проекте.

## Установка

```bash
curl -fsSL https://coasts.dev/install | sh
coast daemon install
```

*Если вы решите не запускать `coast daemon install`, вы несёте ответственность за ручной запуск демона с помощью `coast daemon start` каждый раз.*

## Требования

- macOS
- Docker Desktop
- Проект, использующий Git
- Node.js
- `socat` (`brew install socat` на macOS)

```text
Примечание для Linux: Мы ещё не тестировали Coasts на Linux, но поддержка Linux запланирована.
Вы можете попробовать запустить Coasts на Linux уже сегодня, но мы не даём гарантий, что он будет работать корректно.
```

## Настройка Coasts в проекте

Добавьте Coastfile в корень вашего проекта. Убедитесь, что вы не находитесь в worktree во время установки.

```text
my-project/
├── Coastfile              <-- это то, что читает Coast
├── docker-compose.yml
├── Dockerfile
├── src/
│   └── ...
└── ...
```

`Coastfile` указывает на ваши существующие локальные ресурсы для разработки и добавляет специфичную для Coasts конфигурацию — полный формат см. в [документации Coastfiles](coastfiles/README.md):

```toml
[coast]
name = "my-project"
compose = "./docker-compose.yml"

[ports]
web = 3000
db = 5432
```

Coastfile — это лёгкий TOML-файл, который *обычно* указывает на ваш существующий `docker-compose.yml` (он также работает и с неконтейнеризованными локальными dev-настройками) и описывает изменения, необходимые для параллельного запуска вашего проекта — сопоставления портов, стратегии томов и секреты. Разместите его в корне проекта.

Самый быстрый способ создать Coastfile для вашего проекта — поручить это вашему агенту для кодинга.

CLI Coasts поставляется со встроенным промптом, который обучает любого AI-агента полной схеме Coastfile и CLI. Посмотреть его можно здесь: [installation_prompt.txt](installation_prompt.txt)

Передайте его напрямую вашему агенту или скопируйте [installation prompt](installation_prompt.txt) и вставьте в чат вашего агента:

```bash-emphasis
# Claude Code
claude -p "$(coast installation-prompt)"

# Codex
codex "$(coast installation-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast installation-prompt)"
```

Промпт охватывает TOML-формат Coastfile, стратегии томов, внедрение секретов и все релевантные CLI-команды. Ваш агент проанализирует проект и сгенерирует Coastfile.

## Ваш первый Coast

Перед запуском вашего первого Coast остановите любую уже запущенную среду разработки. Если вы используете Docker Compose, выполните `docker-compose down`. Если у вас запущены локальные dev-серверы — остановите их. Coasts управляют собственными портами и будут конфликтовать со всем, что уже слушает.

Когда ваш Coastfile готов:

```bash
coast build
coast run dev-1
```

Проверьте, что ваш экземпляр запущен:

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

Каждый экземпляр получает собственный набор динамических портов, поэтому несколько экземпляров могут работать бок о бок. Чтобы сопоставить экземпляр с каноническими портами вашего проекта, сделайте checkout:

```bash
coast checkout dev-1
```

Это означает, что runtime теперь находится в состоянии checkout, и канонические порты вашего проекта (например, `3000`, `5432`) будут маршрутизироваться на этот экземпляр Coast.

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -         ✓   ~/dev/my-project
```

Чтобы поднять UI наблюдаемости Coastguard для вашего проекта:

```bash
coast ui
```

## Что дальше?

- Настройте [skill для вашего host-агента](SKILLS_FOR_HOST_AGENTS.md), чтобы он знал, как взаимодействовать с Coasts
