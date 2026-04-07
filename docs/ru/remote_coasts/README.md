# Удалённые побережья

> **Бета.** Удалённые побережья полностью функциональны, но флаги CLI, схема Coastfile и API coast-service могут измениться в будущих выпусках. Если вы обнаружите баг или дефект, пожалуйста, откройте pull request или создайте issue.

Удалённые побережья запускают ваши сервисы на удалённой машине, сохраняя при этом опыт разработки идентичным локальным побережьям. `coast run`, `coast assign`, `coast exec`, `coast ps`, `coast logs` и все остальные команды работают одинаково. Демон определяет, что экземпляр является удалённым, и прозрачно направляет операции через SSH-туннель.

## Почему удалённые

Локальные побережья запускают всё на вашем ноутбуке. Каждый экземпляр побережья запускает полноценный контейнер Docker-in-Docker со всем вашим стеком compose: веб-сервер, API, воркеры, базы данных, кэши, почтовый сервер. Это работает до тех пор, пока на вашем ноутбуке не закончится RAM или место на диске.

Полноценный full-stack проект с несколькими сервисами может потреблять значительный объём RAM на каждое побережье. Запустите несколько побережий параллельно — и упрётесь в пределы вашего ноутбука.

```text
  coast-1         coast-2         coast-3         coast-4
  ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
  │ worker   │   │ worker   │   │ worker   │   │ worker   │
  │ api      │   │ api      │   │ api      │   │ api      │
  │ admin    │   │ admin    │   │ admin    │   │ admin    │
  │ web      │   │ web      │   │ web      │   │ web      │
  │ mailhog  │   │ mailhog  │   │ mailhog  │   │ mailhog  │
  │          │   │          │   │          │   │          │
  │ 12 GB    │   │ 12 GB    │   │ 12 GB    │   │ 12 GB    │
  └──────────┘   └──────────┘   └──────────┘   └──────────┘

  Total: 48 GB RAM on your laptop
```

Удалённые побережья позволяют горизонтально масштабироваться, перенося часть ваших побережий на удалённые машины. Контейнеры DinD, сервисы compose и сборки образов выполняются удалённо, а ваш редактор и агенты остаются локально. Общие сервисы, такие как Postgres и Redis, также остаются локальными, сохраняя вашу базу данных синхронизированной между локальными и удалёнными экземплярами через обратные SSH-туннели.

```text
  Your Machine                         Remote Server
  ┌─────────────────────┐             ┌─────────────────────────┐
  │  editor + agents    │             │  coast-1 (all services) │
  │                     │  SSH        │  coast-2 (all services) │
  │  shared services    │──tunnels──▶ │  coast-3 (all services) │
  │  (postgres, redis)  │             │  coast-4 (all services) │
  └─────────────────────┘             └─────────────────────────┘

  Laptop: lightweight                  Server: 64 GB RAM, 16 CPU
```

Горизонтально масштабируйте вашу среду выполнения localhost.

## Быстрый старт

```bash
# 1. Register a remote machine
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
coast remote test my-vm

# 2. Build on the remote (uses remote's native architecture)
coast build --type remote

# 3. Run a remote coast
coast run dev-1 --type remote

# 4. Everything works as usual
coast ps dev-1
coast exec dev-1 -- bash
coast assign dev-1 --worktree feature/x
coast checkout dev-1
```

Для полных инструкций по настройке, включая подготовку хоста и развёртывание coast-service, см. [Setup](SETUP.md).

## Справочник

| Page | What it covers |
|------|----------------|
| [Architecture](ARCHITECTURE.md) | Разделение на два контейнера (shell coast + remote coast), слой SSH-туннелей, цепочка проброса портов и то, как демон маршрутизирует запросы |
| [Setup](SETUP.md) | Требования к хосту, развёртывание coast-service, регистрация удалённых машин и сквозной быстрый старт |
| [File Sync](FILE_SYNC.md) | rsync для массовой передачи, mutagen для непрерывной синхронизации, жизненный цикл через run/assign/stop, исключения и обработка race condition |
| [Builds](BUILDS.md) | Сборка на удалённой машине для нативной архитектуры, передача артефактов, симлинк `latest-remote`, повторное использование архитектуры и автоочистка |
| [CLI and Configuration](CLI.md) | Команды `coast remote`, конфигурация `Coastfile.remote`, управление диском и `coast remote prune` |

## См. также

- [Remotes](../concepts_and_terminology/REMOTES.md) -- обзор концепции в глоссарии терминов
- [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) -- как локальные общие сервисы пробрасываются в удалённые побережья через обратные туннели
- [Ports](../concepts_and_terminology/PORTS.md) -- как слой SSH-туннелей вписывается в каноническую/динамическую модель портов
