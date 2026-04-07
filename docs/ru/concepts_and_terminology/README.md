# Концепции и терминология

В этом разделе рассматриваются основные концепции и терминология, используемые во всём Coasts. Если вы впервые работаете с Coasts, начните отсюда, прежде чем переходить к конфигурации или расширенным сценариям использования.

- [Coasts](COASTS.md) - самодостаточные среды выполнения вашего проекта, каждая со своими портами, томами и назначением worktree.
- [Run](RUN.md) - создание нового экземпляра Coast из последней сборки с возможным назначением worktree.
- [Remove](REMOVE.md) - удаление экземпляра Coast и его изолированного состояния среды выполнения, когда нужно создать всё заново с нуля или остановить Coasts.
- [Filesystem](FILESYSTEM.md) - общий mount между хостом и Coast, агентами на стороне хоста и переключением worktree.
- [Private Paths](PRIVATE_PATHS.md) - изоляция путей рабочего пространства для каждого экземпляра при конфликтах в общих bind mount.
- [Coast Daemon](DAEMON.md) - локальная управляющая плоскость `coastd`, выполняющая операции жизненного цикла.
- [Coast CLI](CLI.md) - интерфейс командной строки для команд, скриптов и рабочих процессов агентов.
- [Coastguard](COASTGUARD.md) - веб-интерфейс, запускаемый с помощью `coast ui`, для наблюдаемости и управления.
- [Ports](PORTS.md) - канонические порты и динамические порты, а также то, как checkout переключает их.
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - быстрые ссылки на ваш основной сервис, маршрутизация по поддоменам для изоляции cookie и шаблоны URL.
- [Assign and Unassign](ASSIGN.md) - переключение Coast между worktree и доступные стратегии назначения.
- [Checkout](CHECKOUT.md) - сопоставление канонических портов экземпляру Coast и случаи, когда это необходимо.
- [Lookup](LOOKUP.md) - определение того, какие экземпляры Coast соответствуют текущему worktree агента.
- [Volume Topology](VOLUMES.md) - общие сервисы, общие тома, изолированные тома и создание snapshot.
- [Shared Services](SHARED_SERVICES.md) - инфраструктурные сервисы, управляемые хостом, и устранение неоднозначности томов.
- [Secrets and Extractors](SECRETS.md) - извлечение секретов хоста и внедрение их в контейнеры Coast.
- [Builds](BUILDS.md) - анатомия сборки coast, где хранятся артефакты, автоочистка и типизированные сборки.
- [Coastfile Types](COASTFILE_TYPES.md) - компонуемые варианты Coastfile с extends, unset, omit и autostart.
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) - среда выполнения DinD, архитектура Docker-in-Docker и то, как сервисы запускаются внутри Coast.
- [Bare Services](BARE_SERVICES.md) - запуск неконтейнеризированных процессов внутри Coast и почему вместо этого стоит использовать контейнеризацию.
- [Bare Service Optimization](BARE_SERVICE_OPTIMIZATION.md) - условные установки, кэширование, private_paths, подключение к общим сервисам и стратегии назначения для bare services.
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - автоматически внедряемые переменные `<SERVICE>_DYNAMIC_PORT` и способы их использования в командах сервисов.
- [Logs](LOGS.md) - чтение логов сервисов изнутри Coast, компромисс MCP и просмотрщик логов Coastguard.
- [Exec & Docker](EXEC_AND_DOCKER.md) - выполнение команд внутри Coast и работа с внутренним демоном Docker.
- [Agent Shells](AGENT_SHELLS.md) - контейнеризированные TUI агентов, компромисс OAuth и почему, вероятно, лучше запускать агентов на хосте.
- [MCP Servers](MCP_SERVERS.md) - настройка инструментов MCP внутри Coast для контейнеризированных агентов, внутренние серверы и серверы, проксируемые с хоста.
- [Remotes](REMOTES.md) - запуск сервисов на удалённой машине через coast-service с сохранением неизменного локального рабочего процесса.
- [Troubleshooting](TROUBLESHOOTING.md) - doctor, перезапуск демона, удаление проекта и радикальный вариант factory-reset.
