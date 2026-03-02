# Концепции и терминология

Этот раздел охватывает ключевые концепции и словарь, используемые во всём Coasts. Если вы новичок в Coasts, начните здесь, прежде чем переходить к конфигурации или расширенному использованию.

- [Coasts](COASTS.md) — самодостаточные рантаймы вашего проекта, каждый со своими портами, томами и назначением worktree.
- [Filesystem](FILESYSTEM.md) — общая точка монтирования между хостом и Coast, хостовые агенты и переключение worktree.
- [Coast Daemon](DAEMON.md) — локальная control plane `coastd`, выполняющая операции жизненного цикла.
- [Coast CLI](CLI.md) — терминальный интерфейс для команд, скриптов и рабочих процессов агентов.
- [Coastguard](COASTGUARD.md) — веб-интерфейс, запускаемый с помощью `coast ui`, для наблюдаемости и управления.
- [Ports](PORTS.md) — канонические порты vs динамические порты и как checkout переключает между ними.
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) — быстрые ссылки на ваш основной сервис, маршрутизация по поддоменам для изоляции cookie и шаблоны URL.
- [Assign and Unassign](ASSIGN.md) — переключение Coast между worktree и доступные стратегии назначения.
- [Checkout](CHECKOUT.md) — сопоставление канонических портов с экземпляром Coast и когда это нужно.
- [Lookup](LOOKUP.md) — определение того, какие экземпляры Coast соответствуют текущему worktree агента.
- [Volume Topology](VOLUMES.md) — общие сервисы, общие тома, изолированные тома и создание снимков.
- [Shared Services](SHARED_SERVICES.md) — управляемые хостом инфраструктурные сервисы и устранение неоднозначности томов.
- [Secrets and Extractors](SECRETS.md) — извлечение секретов с хоста и внедрение их в контейнеры Coast.
- [Builds](BUILDS.md) — анатомия сборки coast, где живут артефакты, автоочистка и типизированные сборки.
- [Coastfile Types](COASTFILE_TYPES.md) — компонуемые варианты Coastfile с extends, unset, omit и autostart.
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) — рантайм DinD, архитектура Docker-in-Docker и то, как сервисы работают внутри Coast.
- [Bare Services](BARE_SERVICES.md) — запуск неконтейнеризованных процессов внутри Coast и почему вместо этого следует контейнеризовать.
- [Logs](LOGS.md) — чтение логов сервисов изнутри Coast, компромисс MCP и просмотрщик логов Coastguard.
- [Exec & Docker](EXEC_AND_DOCKER.md) — выполнение команд внутри Coast и взаимодействие с внутренним демоном Docker.
- [Agent Shells](AGENT_SHELLS.md) — контейнеризованные TUI агентов, компромисс OAuth и почему, вероятно, лучше запускать агентов на хосте.
- [MCP Servers](MCP_SERVERS.md) — настройка инструментов MCP внутри Coast для контейнеризованных агентов, внутренние серверы vs проксируемые через хост.
- [Troubleshooting](TROUBLESHOOTING.md) — doctor, перезапуск демона, удаление проекта и вариант «ядерного» factory-reset.
