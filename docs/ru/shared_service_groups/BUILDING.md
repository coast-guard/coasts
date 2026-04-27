# Сборка Shared Service Group

`coast ssg build` разбирает `Coastfile.shared_service_groups` вашего проекта, извлекает все объявленные секреты, загружает каждый образ в кэш образов хоста и записывает версионированный артефакт сборки в `~/.coast/ssg/<project>/builds/<build_id>/`. Команда не разрушает уже запущенный SSG — следующий `coast ssg run` или `coast ssg start` подхватит новую сборку, но работающий `<project>-ssg` продолжит обслуживать свою текущую сборку, пока вы не перезапустите его.

Имя проекта берётся из `[coast].name` в соседнем `Coastfile`. У каждого проекта есть собственный SSG с именем `<project>-ssg`, собственный каталог сборок и собственный `latest_build_id` — никакого общесистемного «текущего SSG» не существует.

Полную TOML-схему см. в [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md).

## Обнаружение

`coast ssg build` находит свой Coastfile по тем же правилам, что и `coast build`:

- Без флагов он ищет в текущем рабочем каталоге `Coastfile.shared_service_groups` или `Coastfile.shared_service_groups.toml`. Обе формы эквивалентны, и суффикс `.toml` имеет приоритет, если существуют оба файла.
- `-f <path>` / `--file <path>` указывает на произвольный файл.
- `--working-dir <dir>` отделяет корень проекта от расположения Coastfile (тот же флаг, что и `coast build --working-dir`).
- `--config '<inline-toml>'` поддерживает сценарии и CI-потоки, где вы синтезируете Coastfile прямо в строке.

```bash
coast ssg build
coast ssg build -f /path/to/Coastfile.shared_service_groups
coast ssg build --working-dir /shared/coast
coast ssg build --config '[shared_services.pg]
image = "postgres:16"
ports = [5432]'
```

Сборка определяет имя проекта из соседнего `Coastfile` в том же каталоге. Если вы используете `--config` (без `Coastfile.shared_service_groups` на диске), текущий каталог всё равно должен содержать `Coastfile`, у которого `[coast].name` является проектом SSG.

## Что делает Build

Каждый `coast ssg build` передаёт прогресс через тот же канал `BuildProgressEvent`, что и `coast build`, поэтому CLI отображает счётчики шагов `[N/M]`.

1. **Разбирает** `Coastfile.shared_service_groups`. `[ssg]`, `[shared_services.*]`, `[secrets.*]` и `[unset]` — допустимые секции верхнего уровня. Записи томов разделяются на bind mount’ы хоста и внутренние именованные тома (см. [Volumes](VOLUMES.md)).
2. **Определяет build id.** Идентификатор имеет вид `{coastfile_hash}_{YYYYMMDDHHMMSS}`. Хэш включает исходный текст, детерминированное резюме разобранных сервисов и конфигурацию `[secrets.*]` (так что изменение `extractor` или `var` у секрета создаёт новый id).
3. **Синтезирует внутренний `compose.yml`.** Каждый блок `[shared_services.*]` превращается в запись в единственном Docker Compose-файле. Именно этот файл внутренний Docker daemon SSG запускает через `docker compose up -d` во время `coast ssg run`.
4. **Извлекает секреты.** Когда `[secrets.*]` не пуст, запускает каждый объявленный extractor и сохраняет зашифрованный результат в `~/.coast/keystore.db` под `coast_image = "ssg:<project>"`. Тихо пропускается, если в Coastfile нет блока `[secrets]`. Полный конвейер см. в [Secrets](SECRETS.md).
5. **Загружает и кэширует каждый образ.** Образы сохраняются как OCI tarball’ы в `~/.coast/image-cache/`, тот же пул использует `coast build`. Попадания в кэш из любой команды ускоряют другую.
6. **Записывает артефакт сборки** в `~/.coast/ssg/<project>/builds/<build_id>/` с тремя файлами: `manifest.json`, `ssg-coastfile.toml` и `compose.yml` (см. структуру ниже).
7. **Обновляет `latest_build_id` проекта.** Это флаг в базе состояния, а не симлинк в файловой системе. `coast ssg run` и `coast ssg ps` читают его, чтобы понимать, с какой сборкой работать.
8. **Автоматически очищает** старые сборки, оставляя 5 самых новых для этого проекта. Более ранние каталоги артефактов в `~/.coast/ssg/<project>/builds/` удаляются с диска. Закреплённые сборки (см. «Locking a project to a specific build» ниже) сохраняются всегда.

## Структура артефактов

```text
~/.coast/
  keystore.db                                          (shared, namespaced by coast_image)
  keystore.key
  image-cache/                                         (shared OCI tarball pool)
  ssg/
    cg/                                                (project "cg")
      builds/
        b455787d95cfdeb_20260420061903/                (the new build)
          manifest.json
          ssg-coastfile.toml
          compose.yml
        a1c7d783e4f56c9a_20260419184221/               (prior build)
          ...
    filemap/                                           (project "filemap" -- separate tree)
      builds/
        ...
    runs/
      cg/                                              (per-project run scratch)
        compose.override.yml                           (rendered at coast ssg run)
        secrets/<basename>                             (file-injected secrets, mode 0600)
```

`manifest.json` фиксирует метаданные сборки, которые важны для последующего кода:

```json
{
  "build_id": "b455787d95cfdeb_20260420061903",
  "built_at": "2026-04-20T06:19:03Z",
  "coastfile_hash": "b455787d95cfdeb",
  "services": [
    {
      "name": "postgres",
      "image": "postgres:16",
      "ports": [5432],
      "env_keys": ["POSTGRES_USER", "POSTGRES_DB"],
      "volumes": ["pg_data:/var/lib/postgresql/data"],
      "auto_create_db": true
    }
  ],
  "secret_injects": [
    {
      "secret_name": "pg_password",
      "inject_type": "env",
      "inject_target": "POSTGRES_PASSWORD",
      "services": ["postgres"]
    }
  ]
}
```

Значения env и содержимое секретов намеренно отсутствуют — фиксируются только имена переменных окружения и *targets* инъекций. Значения секретов хранятся в keystore в зашифрованном виде, никогда не в файлах артефактов.

`ssg-coastfile.toml` — это разобранный, интерполированный, прошедший валидацию Coastfile. Он побайтно идентичен тому, что daemon видел бы во время разбора. Полезно для аудита прошлой сборки.

`compose.yml` — это то, что запускает внутренний Docker daemon SSG. Правила синтеза, особенно стратегию bind mount’ов с симметричными путями, см. в [Volumes](VOLUMES.md).

## Просмотр сборки без её запуска

`coast ssg ps` напрямую читает `manifest.json` для `latest_build_id` проекта — он не инспектирует никакой контейнер. Вы можете запустить его сразу после `coast ssg build`, чтобы увидеть сервисы, которые стартуют при следующем `coast ssg run`:

```bash
coast ssg ps

# SSG build: b455787d95cfdeb_20260420061903 (project: cg)
#
#   SERVICE              IMAGE                          PORT       STATUS
#   postgres             postgres:16                    5432       built
#   redis                redis:7-alpine                 6379       built
```

Столбец `PORT` — это внутренний порт контейнера. Динамические порты хоста выделяются во время `coast ssg run`; виртуальный порт, видимый потребителю, сообщает `coast ssg ports`. Полную картину см. в [Routing](ROUTING.md).

Чтобы просмотреть все сборки проекта (с временными метками, количеством сервисов и указанием, какая сборка сейчас latest), используйте:

```bash
coast ssg builds-ls
```

## Повторные сборки

Новый `coast ssg build` — канонический способ обновить SSG. Он заново извлекает секреты (если они есть), обновляет `latest_build_id` и очищает старые артефакты. Потребители не пересобираются автоматически — их ссылки `from_group = true` разрешаются во время сборки потребителя относительно той сборки, которая была текущей на тот момент. Чтобы перевести потребителя на более новый SSG, выполните `coast build` для потребителя.

Во время выполнения система терпима к повторным сборкам: виртуальные порты остаются стабильными для каждого `(project, service, container_port)`, поэтому потребителей не нужно обновлять ради маршрутизации. Изменения формы (сервис был переименован или удалён) проявляются как ошибки соединения на уровне потребителя, а не как сообщение Coast об уровне «drift». Почему так — см. в [Routing](ROUTING.md).

## Закрепление проекта за конкретной сборкой

По умолчанию SSG запускает `latest_build_id` проекта. Если вам нужно зафиксировать проект на более ранней сборке — для воспроизведения регрессии, A/B-сравнения двух сборок между worktree или удержания долгоживущей ветки на заведомо рабочей форме — используйте команды pin:

```bash
coast ssg checkout-build <build_id>     # pin this project to <build_id>
coast ssg show-pin                      # report the active pin (if any)
coast ssg uncheckout-build              # release the pin; back to latest
```

Закрепления действуют на уровне проекта-потребителя (одно закрепление на проект, общее для всех worktree). Когда сборка закреплена:

- `coast ssg run` автоматически запускает закреплённую сборку вместо `latest_build_id`.
- `coast build` валидирует ссылки `from_group` по манифесту закреплённой сборки.
- `auto_prune` не удалит каталог закреплённой сборки, даже если он выходит за пределы окна из 5 самых новых.

Coastguard SPA показывает бейдж `PINNED` рядом с build id, когда закрепление активно, и `LATEST`, когда его нет. Команды pin также перечислены в [CLI](CLI.md).
