# Справочник CLI `coast ssg`

Каждая подкоманда `coast ssg` взаимодействует с одним и тем же локальным демоном через существующий Unix-сокет. `coast shared-service-group` — это алиас для `coast ssg`.

Большинство глаголов определяют проект из `[coast].name` в `Coastfile` текущего каталога (или `--working-dir <dir>`). Только `coast ssg ls` работает между проектами.

Все команды принимают глобальный флаг `--silent` / `-s`, который подавляет вывод прогресса и печатает только итоговую сводку или ошибки.

## Команды

### Сборка и просмотр

| Command | Summary |
|---------|---------|
| `coast ssg build [-f <file>] [--working-dir <dir>] [--config '<toml>']` | Разобрать `Coastfile.shared_service_groups`, извлечь все `[secrets.*]`, скачать образы, записать артефакт в `~/.coast/ssg/<project>/builds/<id>/`, обновить `latest_build_id`, удалить старые сборки. См. [Building](BUILDING.md). |
| `coast ssg ps` | Показать список сервисов SSG-сборки этого проекта (читает `manifest.json` плюс живое состояние контейнеров). См. [Lifecycle -> ps](LIFECYCLE.md#coast-ssg-ps). |
| `coast ssg builds-ls [--working-dir <dir>] [-f <file>]` | Перечислить все артефакты сборок в `~/.coast/ssg/<project>/builds/` с временной меткой, количеством сервисов и пометками `(latest)` / `(pinned)`. |
| `coast ssg ls` | Межпроектный список всех SSG, известных демону (проект, статус, id сборки, количество сервисов, время создания). См. [Lifecycle -> ls](LIFECYCLE.md#coast-ssg-ls). |

### Жизненный цикл

| Command | Summary |
|---------|---------|
| `coast ssg run` | Создать DinD `<project>-ssg`, выделить динамические host-порты, материализовать секреты (если объявлены), запустить внутренний compose-стек. См. [Lifecycle -> run](LIFECYCLE.md#coast-ssg-run). |
| `coast ssg start` | Запустить ранее созданный, но остановленный SSG. Повторно материализует секреты и заново поднимает все сохранённые socat для checkout на канонических портах. |
| `coast ssg stop [--force]` | Остановить DinD SSG проекта. Сохраняет контейнер, динамические порты, виртуальные порты и записи checkout. `--force` сначала разрывает удалённые SSH-туннели. |
| `coast ssg restart` | Остановить + запустить. Сохраняет контейнер и динамические порты. |
| `coast ssg rm [--with-data] [--force]` | Удалить DinD SSG проекта. `--with-data` удаляет внутренние именованные тома. `--force` продолжает, несмотря на удалённых shadow-потребителей. Содержимое host bind-mount никогда не затрагивается. **Keystore никогда не затрагивается** — для этого используйте `coast ssg secrets clear`. |

### Логи и exec

| Command | Summary |
|---------|---------|
| `coast ssg logs [--service <name>] [--tail N] [--follow]` | Потоково выводить логи внешнего DinD или одного внутреннего сервиса. `--follow` продолжает поток до Ctrl+C. |
| `coast ssg exec [--service <name>] -- <cmd...>` | Выполнить exec во внешний контейнер `<project>-ssg` или в один внутренний сервис. Всё после `--` передаётся дословно. |

### Маршрутизация и checkout

| Command | Summary |
|---------|---------|
| `coast ssg ports` | Показать для каждого сервиса сопоставление канонических / динамических / виртуальных портов с пометкой `(checked out)` там, где применимо. См. [Routing](ROUTING.md). |
| `coast ssg checkout [--service <name> \| --all]` | Привязать канонические host-порты через socat на стороне host (forwarder направляется на стабильный виртуальный порт проекта). Вытесняет держателей Coast-instance с предупреждением; выдаёт ошибку для неизвестных host-процессов. См. [Checkout](CHECKOUT.md). |
| `coast ssg uncheckout [--service <name> \| --all]` | Удалить socat на канонических портах для этого проекта. Не восстанавливает автоматически вытесненные Coast. |

### Диагностика

| Command | Summary |
|---------|---------|
| `coast ssg doctor` | Проверка только для чтения прав host bind-mount для сервисов с известными образами и объявленных, но не извлечённых секретов SSG. Выдаёт результаты `ok` / `warn` / `info`. См. [Volumes -> coast ssg doctor](VOLUMES.md#coast-ssg-doctor). |

### Закрепление сборки

| Command | Summary |
|---------|---------|
| `coast ssg checkout-build <BUILD_ID> [--working-dir <dir>] [-f <file>]` | Закрепить SSG этого проекта за конкретным `build_id`. `coast ssg run` и `coast build` используют закреплённую сборку вместо `latest_build_id`. См. [Building -> Locking a project to a specific build](BUILDING.md#locking-a-project-to-a-specific-build). |
| `coast ssg uncheckout-build [--working-dir <dir>] [-f <file>]` | Снять закрепление. Идемпотентно. |
| `coast ssg show-pin [--working-dir <dir>] [-f <file>]` | Показать текущее закрепление для этого проекта, если оно есть. |

### SSG-нативные секреты

| Command | Summary |
|---------|---------|
| `coast ssg secrets clear` | Удалить все записи зашифрованного keystore в `coast_image = "ssg:<project>"`. Идемпотентно. Это единственная команда, которая стирает SSG-нативные секреты — `coast ssg rm` и `rm --with-data` намеренно их не трогают. См. [Secrets](SECRETS.md). |

### Помощник миграции

| Command | Summary |
|---------|---------|
| `coast ssg import-host-volume <VOLUME> --service <name> --mount <path> [--apply] [-f <file>] [--working-dir <dir>] [--config '<toml>']` | Определить точку монтирования именованного host Docker volume и вывести (или применить) эквивалентную запись SSG bind-mount. См. [Volumes -> coast ssg import-host-volume](VOLUMES.md#automating-the-recipe-coast-ssg-import-host-volume). |

## Коды выхода

- `0` -- успех. Такие команды, как `doctor`, возвращают 0 даже при обнаружении предупреждений; это диагностические инструменты, а не блокирующие проверки.
- Ненулевой -- ошибка валидации, ошибка Docker, несогласованность состояния или отказ из-за remote-shadow gate.

## См. также

- [Building](BUILDING.md)
- [Lifecycle](LIFECYCLE.md)
- [Routing](ROUTING.md)
- [Volumes](VOLUMES.md)
- [Consuming](CONSUMING.md)
- [Secrets](SECRETS.md)
- [Checkout](CHECKOUT.md)
