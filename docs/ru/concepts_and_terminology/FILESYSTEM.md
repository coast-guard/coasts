# Файловая система

Ваша хост-машина и каждый экземпляр Coast используют одни и те же файлы проекта. Корень проекта на хосте монтируется на чтение-запись в контейнер DinD по пути `/host-project`, а Coast bind-монтирует активное рабочее дерево по пути `/workspace`. Именно это позволяет агенту, запущенному на хост-машине, редактировать код, а сервисам внутри Coast подхватывать изменения в реальном времени.

## Общее монтирование

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

Корень проекта на хосте монтируется на чтение-запись по пути `/host-project` внутри [контейнера DinD](RUNTIMES_AND_SERVICES.md) при создании контейнера. После запуска контейнера команда `mount --bind /host-project /workspace` внутри контейнера создаёт рабочий путь `/workspace` с общей пропагацией монтирований (`mount --make-rshared`), чтобы внутренние compose-сервисы, которые bind-монтируют подкаталоги `/workspace`, видели корректное содержимое.

Этот двухэтапный подход существует не просто так: Docker bind mount в `/host-project` фиксируется при создании контейнера и не может быть изменён без пересоздания контейнера. Но Linux bind mount в `/workspace` внутри контейнера можно размонтировать и примонтировать заново к другому подкаталогу — worktree — не затрагивая жизненный цикл контейнера. Именно это делает `coast assign` быстрым.

`/workspace` доступен на чтение-запись. Изменения файлов мгновенно распространяются в обе стороны. Сохраните файл на хосте — и dev-сервер внутри Coast тут же подхватит его. Создайте файл внутри Coast — и он появится на хосте.

## Агенты на хосте и Coast

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

Поскольку файловая система общая, ИИ-агент для написания кода, запущенный на хосте, может свободно редактировать файлы, и работающие сервисы внутри Coast немедленно видят изменения. Агенту не нужно запускаться внутри контейнера Coast — он работает с хоста как обычно.

Когда агенту нужна информация времени выполнения — логи, статус сервисов, вывод тестов — он вызывает команды Coast CLI с хоста:

- `coast logs dev-1 --service web --tail 50` для вывода сервиса (см. [Logs](LOGS.md))
- `coast ps dev-1` для статуса сервисов (см. [Runtimes and Services](RUNTIMES_AND_SERVICES.md))
- `coast exec dev-1 -- npm test` чтобы запускать команды внутри Coast (см. [Exec & Docker](EXEC_AND_DOCKER.md))

Это фундаментальное архитектурное преимущество: **редактирование кода происходит на хосте, выполнение — в Coast, а общая файловая система связывает их.** Хост-агенту никогда не нужно быть «внутри» Coast, чтобы выполнять свою работу.

## Переключение worktree

Когда `coast assign` переключает Coast на другое worktree, он перемонтирует `/workspace`, чтобы он указывал на это git worktree вместо корня проекта:

```text
coast assign dev-1 --worktree feature-auth

Before:  /workspace  ←──mount──  /host-project                          (project root)
After:   /workspace  ←──mount──  /host-project/.worktrees/feature-auth   (worktree)
```

Worktree создаётся на хосте в `{project_root}/.worktrees/{worktree_name}`. Имя каталога `.worktrees` настраивается через `worktree_dir` в вашем Coastfile и должно быть в вашем `.gitignore`.

Если worktree новый, Coast перед перемонтированием подготавливает выбранные gitignored-файлы из корня проекта. Он перечисляет игнорируемые файлы с помощью `git ls-files --others --ignored --exclude-standard`, отфильтровывает типичные «тяжёлые» каталоги плюс любые настроенные `exclude_paths`, затем использует `rsync --files-from` с `--link-dest`, чтобы создать жёсткие ссылки на выбранные файлы в worktree. Coast записывает эту подготовку во внутренние метаданные worktree и пропускает её при последующих assign к тому же worktree, если только вы явно не обновите её с помощью `coast assign --force-sync`.

Внутри контейнера `/workspace` «лениво» размонтируется и привязывается заново к подкаталогу worktree по пути `/host-project/.worktrees/{branch_name}`. Это перемонтирование быстрое — оно не пересоздаёт контейнер DinD и не перезапускает внутренний Docker daemon. Compose- и bare-сервисы всё же могут быть пересозданы или перезапущены после перемонтирования, чтобы их bind-монтирования разрешались через новый `/workspace`.

Большие каталоги зависимостей, такие как `node_modules`, не входят в этот общий путь подготовки. Обычно они обрабатываются через специфичные для сервисов кэши или тома.

Если вы используете `[assign.rebuild_triggers]`, Coast также запускает `git diff --name-only <previous>..<worktree>` на хосте, чтобы решить, можно ли сервис, помеченный `rebuild`, понизить до `restart`. Подробности, влияющие на задержку assign, см. в [Assign and Unassign](ASSIGN.md) и [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md).

`coast unassign` возвращает `/workspace` обратно к `/host-project` (корню проекта). `coast start` после остановки повторно применяет корректное монтирование в зависимости от того, назначено ли экземпляру worktree.

## Все монтирования

У каждого контейнера Coast есть следующие монтирования:

| Path | Type | Access | Purpose |
|---|---|---|---|
| `/workspace` | bind mount (in-container) | RW | Корень проекта или worktree. Переключается при assign. |
| `/host-project` | Docker bind mount | RW | «Сырой» корень проекта. Фиксирован при создании контейнера. |
| `/image-cache` | Docker bind mount | RO | Предварительно скачанные OCI tarballs из `~/.coast/image-cache/`. |
| `/coast-artifact` | Docker bind mount | RO | Артефакт сборки с переписанными compose-файлами. |
| `/coast-override` | Docker bind mount | RO | Сгенерированные compose-переопределения для [shared services](SHARED_SERVICES.md). |
| `/var/lib/docker` | Named volume | RW | Состояние внутреннего Docker daemon. Сохраняется при удалении контейнера. |

Монтирования только для чтения — это инфраструктура: они содержат артефакт сборки, кэшированные образы и compose-переопределения, которые генерирует Coast. Вы взаимодействуете с ними косвенно через `coast build` и Coastfile. Монтирования на чтение-запись — это место, где живёт ваш код и где внутренний daemon хранит своё состояние.
