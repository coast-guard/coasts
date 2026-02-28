# Файловая система

Ваша хост-машина и каждый экземпляр Coast используют одни и те же проектные файлы. Корень проекта на хосте bind-монтируется в контейнер DinD по пути `/workspace`, поэтому изменения на хосте мгновенно появляются внутри Coast и наоборот. Это делает возможным, чтобы агент, запущенный на вашей хост-машине, редактировал код, а сервисы внутри Coast подхватывали изменения в реальном времени.

## Общая точка монтирования

```text
Host machine
│
├── ~/dev/my-app/                     (project root)
│   ├── src/
│   ├── Coastfile
│   ├── docker-compose.yml
│   └── .worktrees/                   (worktrees, gitignored)
│       ├── feature-auth/
│       └── feature-billing/
│
└── Docker daemon (host)
    │
    └── Coast: dev-1 (docker:dind)
        │
        ├── /host-project              ← Docker bind mount of project root (RW, fixed)
        │
        ├── /workspace                 ← mount --bind /host-project (switchable)
        │   ├── src/                     same files, same bytes, instant sync
        │   ├── Coastfile
        │   └── docker-compose.yml
        │
        └── Inner Docker daemon
            └── web service
                └── /app               ← compose bind mount from /workspace/src
```

Корень проекта на хосте монтируется на чтение-запись по пути `/host-project` внутри [контейнера DinD](RUNTIMES_AND_SERVICES.md) при создании контейнера. После запуска контейнера, команда внутри контейнера `mount --bind /host-project /workspace` создаёт рабочий путь `/workspace` с общей пропагацией монтирования (`mount --make-rshared`), чтобы внутренние compose-сервисы, которые bind-монтируют подкаталоги `/workspace`, видели корректное содержимое.

Этот двухэтапный подход существует не просто так: Docker bind mount по пути `/host-project` фиксируется при создании контейнера и не может быть изменён без пересоздания контейнера. Но Linux bind mount `/workspace` внутри контейнера можно размонтировать и заново примонтировать к другому подкаталогу — worktree — не затрагивая жизненный цикл контейнера. Именно это делает `coast assign` быстрым.

`/workspace` доступен на чтение-запись. Изменения файлов мгновенно текут в обе стороны. Сохраните файл на хосте — и dev-сервер внутри Coast подхватит его. Создайте файл внутри Coast — и он появится на хосте.

## Хостовые агенты и Coast

```text
┌─── Host machine ──────────────────────────────────────────┐
│                                                           │
│   AI Agent (Cursor, Claude Code, etc.)                    │
│     │                                                     │
│     ├── reads/writes files at <project root>/src/         │
│     │       ↕ (instant, same filesystem)                  │
│     ├── coast logs dev-1 --service web --tail 50          │
│     ├── coast ps dev-1                                    │
│     └── coast exec dev-1 -- npm test                      │
│                                                           │
├───────────────────────────────────────────────────────────┤
│                                                           │
│   Coast: dev-1                                            │
│     └── /workspace/src/  ← same bytes as host project/src │
│         └── web service picks up changes on save          │
│                                                           │
└───────────────────────────────────────────────────────────┘
```

Поскольку файловая система общая, AI-агент для написания кода, запущенный на хосте, может свободно редактировать файлы, а запущенные сервисы внутри Coast сразу видят изменения. Агенту не нужно запускаться внутри контейнера Coast — он работает с хоста как обычно.

Когда агенту нужна информация времени выполнения — логи, статус сервисов, вывод тестов — он вызывает команды Coast CLI с хоста:

- `coast logs dev-1 --service web --tail 50` для вывода сервиса (см. [Logs](LOGS.md))
- `coast ps dev-1` для статуса сервисов (см. [Runtimes and Services](RUNTIMES_AND_SERVICES.md))
- `coast exec dev-1 -- npm test` чтобы запускать команды внутри Coast (см. [Exec & Docker](EXEC_AND_DOCKER.md))

Это фундаментальное архитектурное преимущество: **редактирование кода происходит на хосте, выполнение — в Coast, а общая файловая система связывает их.** Хостовому агенту никогда не нужно быть «внутри» Coast, чтобы выполнять свою работу.

## Переключение worktree

Когда `coast assign` переключает Coast на другой worktree, он перемонтирует `/workspace`, чтобы указывать на этот git worktree вместо корня проекта:

```text
coast assign dev-1 --worktree feature-auth

Before:  /workspace  ←──mount──  /host-project                          (project root)
After:   /workspace  ←──mount──  /host-project/.worktrees/feature-auth   (worktree)
```

Worktree создаётся на хосте по пути `{project_root}/.worktrees/{worktree_name}`. Имя каталога `.worktrees` настраивается через `worktree_dir` в вашем Coastfile и должно быть добавлено в `.gitignore`.

Внутри контейнера `/workspace` «лениво» размонтируется и заново привязывается к подкаталогу worktree по пути `/host-project/.worktrees/{branch_name}`. Это перемонтирование быстрое — оно не пересоздаёт контейнер DinD и не перезапускает внутренний Docker daemon. Внутренние compose-сервисы пересоздаются, чтобы их bind-монты разрешались через новый `/workspace`.

Gitignored-файлы, такие как `node_modules`, синхронизируются из корня проекта в worktree через rsync с hardlink'ами, поэтому начальная настройка почти мгновенная даже для больших деревьев зависимостей.

На macOS операции ввода-вывода файлов между хостом и Docker VM имеют неизбежные накладные расходы. Coast запускает `git ls-files` во время assign и unassign, чтобы диффить worktree, и в больших кодовых базах это может добавлять заметную задержку. Если части вашего проекта не нужно диффить между assign'ами (документация, тестовые фикстуры, скрипты), вы можете исключить их с помощью `exclude_paths` в вашем Coastfile, чтобы снизить эти накладные расходы. Подробности см. в [Assign and Unassign](ASSIGN.md).

`coast unassign` возвращает `/workspace` обратно к `/host-project` (корню проекта). `coast start` после остановки заново применяет корректное монтирование в зависимости от того, назначен ли экземпляру worktree.

## Все монтирования

Каждый контейнер Coast имеет следующие монтирования:

| Path | Type | Access | Purpose |
|---|---|---|---|
| `/workspace` | bind mount (in-container) | RW | Корень проекта или worktree. Переключается при assign. |
| `/host-project` | Docker bind mount | RW | Исходный корень проекта. Фиксируется при создании контейнера. |
| `/image-cache` | Docker bind mount | RO | Предварительно скачанные OCI tarball'ы из `~/.coast/image-cache/`. |
| `/coast-artifact` | Docker bind mount | RO | Артефакт сборки с переписанными compose-файлами. |
| `/coast-override` | Docker bind mount | RO | Сгенерированные compose override'ы для [shared services](SHARED_SERVICES.md). |
| `/var/lib/docker` | Named volume | RW | Состояние внутреннего Docker daemon. Сохраняется при удалении контейнера. |

Монтирования только для чтения — это инфраструктура: они несут артефакт сборки, кэшированные образы и compose override'ы, которые генерирует Coast. Вы взаимодействуете с ними косвенно через `coast build` и Coastfile. Монтирования на чтение-запись — это место, где живёт ваш код и где внутренний daemon хранит своё состояние.
