# Тома SSG

Внутри `[shared_services.<name>]` массив `volumes` использует стандартный синтаксис Docker Compose:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

Начальный `/` означает **путь bind-монтирования хоста** -- байты находятся в файловой системе хоста, а внутренний сервис читает и записывает их напрямую. Без начального слеша, например `pg_wal:/var/lib/postgresql/wal`, источник -- это **именованный том Docker, который находится внутри вложенного демона Docker SSG** -- он сохраняется после `coast ssg rm` и удаляется командой `coast ssg rm --with-data`. Допускаются обе формы.

Отклоняются при разборе: относительные пути (`./data:/...`), компоненты `..`, тома только для контейнера (без источника) и дублирующиеся цели в пределах одного сервиса.

## Повторное использование тома Docker из docker-compose или встроенного общего сервиса

Если у вас уже есть данные внутри именованного тома Docker на хосте -- от `docker-compose up`, от встроенного `[shared_services.postgres] volumes = ["infra_postgres_data:/..."]` или от вручную созданного `docker volume create` -- вы можете заставить SSG читать те же самые байты, примонтировав bind-монтированием базовый каталог тома на хосте:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

Левая сторона -- это путь в файловой системе хоста к существующему тому Docker; `docker volume inspect <name>` сообщает его в поле `Mountpoint`. Coast не копирует байты -- SSG читает и записывает те же файлы, что и docker-compose. `coast ssg rm` (без `--with-data`) не трогает том, поэтому docker-compose тоже может продолжать его использовать.

> **Почему нельзя просто `infra_postgres_data:/var/lib/postgresql/data`?** Это работает для встроенных `[shared_services.*]` (том создаётся в демоне Docker хоста, где его видит docker-compose). Внутри SSG это *не* работает так же -- имя без начального слеша создаёт новый том внутри вложенного демона Docker SSG, изолированный от хоста. Вместо этого используйте путь к точке монтирования тома, если хотите делить данные с чем-либо, что работает в демоне хоста.

### `coast ssg import-host-volume`

`coast ssg import-host-volume` получает `Mountpoint` тома через `docker volume inspect` и выводит (или применяет) эквивалентную строку `volumes`, чтобы вам не приходилось вручную составлять путь `/var/lib/docker/volumes/<name>/_data`.

Режим фрагмента (по умолчанию) печатает TOML-фрагмент для вставки:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data
```

На выходе получается блок `[shared_services.postgres]` с уже добавленной новой записью `volumes = [...]`:

```text
# Add the following to Coastfile.shared_service_groups (infra_postgres_data -> /var/lib/postgresql/data):

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_PASSWORD = "coast" }

# Bind line: /var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data
```

Режим применения переписывает `Coastfile.shared_service_groups` на месте и сохраняет оригинал в `Coastfile.shared_service_groups.bak`:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data \
    --apply
```

Флаги:

- `<VOLUME>` (позиционный) -- именованный том Docker на хосте. Должен уже существовать (проверка выполняется через `docker volume inspect`); в противном случае сначала создайте или переименуйте его с помощью `docker volume create`.
- `--service` -- секция `[shared_services.<name>]` для редактирования. Секция должна уже существовать.
- `--mount` -- абсолютный путь контейнера. Относительные пути отклоняются. Дублирующиеся пути монтирования в одном и том же сервисе считаются жёсткими ошибками.
- `--file` / `--working-dir` / `--config` -- поиск Coastfile SSG, те же правила, что и у `coast ssg build`.
- `--apply` -- переписать Coastfile на месте. Нельзя комбинировать с `--config` (встроенный текст некуда записывать обратно).

Файл `.bak` содержит исходные байты дословно, так что вы можете восстановить точное состояние до применения.

`/var/lib/docker/volumes/<name>/_data` -- это путь, который Docker использует как точку монтирования тома уже много лет, и именно его сегодня сообщает `docker volume inspect`. Docker формально не обещает сохранять этот путь навсегда; если в будущей версии Docker тома будут перемещены в другое место, повторно запустите `coast ssg import-host-volume`, чтобы получить новый путь.

## Права доступа

Некоторые образы отказываются запускаться, если каталог их данных принадлежит неправильному пользователю. Наиболее частые случаи -- Postgres (UID 999 в debian-теге, UID 70 в alpine-теге), MySQL/MariaDB (UID 999) и MongoDB (UID 999). Если каталог на хосте принадлежит root, Postgres завершится при запуске с кратким сообщением «data directory has wrong ownership».

Исправление -- одна команда:

```bash
# postgres:16 (debian)
sudo chown -R 999:999 /var/coast-data/postgres

# postgres:16-alpine
sudo chown -R 70:70 /var/coast-data/postgres
```

Выполните это перед `coast ssg run`. Если каталога ещё нет, `coast ssg run` создаст его со стандартным владельцем (root в Linux, ваш пользователь в macOS через Docker Desktop). Для Postgres такой владелец обычно неверен. Если вы пришли сюда через `coast ssg import-host-volume`, и `docker-compose up` ранее уже выполнил `chown` для тома при первом запуске, то у вас уже всё в порядке.

## `coast ssg doctor`

`coast ssg doctor` -- это проверка только для чтения, которая выполняется для SSG текущего проекта (определяется из `[coast].name` в `Coastfile` текущего каталога или через `--working-dir`). Она печатает по одному результату на каждую пару `(service, host-bind)` в активной сборке, а также результаты извлечения секретов (см. [Secrets](SECRETS.md)).

Для каждого известного образа (Postgres, MySQL, MariaDB, MongoDB) она обращается к встроенной таблице UID/GID, сравнивает её с `stat(2)` для каждого пути хоста и выводит:

- `ok`, когда владелец соответствует ожиданиям образа.
- `warn`, когда есть расхождение. Сообщение включает команду `chown` для исправления.
- `info`, когда каталог ещё не существует или когда у соответствующего образа есть только именованные тома (то есть со стороны хоста нечего проверять).

Сервисы, образы которых отсутствуют в таблице известных образов, молча пропускаются. Форки вроде `ghcr.io/baosystems/postgis` не помечаются -- doctor предпочитает ничего не говорить, чем выдавать неверное предупреждение.

```bash
coast ssg doctor
```

Пример вывода для каталога Postgres с неверным владельцем:

```text
SSG 'b455787d95cfdeb_20260420061903' (project cg): 1 warning(s), 0 ok, 0 info. Fix the warnings before `coast ssg run`.

  LEVEL   SERVICE              PATH                                     MESSAGE
  warn    postgres             /var/coast-data/postgres                 Owner 0:0 but postgres expects 999:999. Run `sudo chown -R 999:999 /var/coast-data/postgres` before `coast ssg run`.
```

Doctor ничего не изменяет. Права доступа к байтам, которые вы разместили в файловой системе хоста, не относятся к тому, что Coast молча модифицирует.

## Примечания по платформам

- **macOS Docker Desktop.** Необработанные пути хоста должны быть перечислены в Settings -> Resources -> File Sharing. По умолчанию туда входят `/Users`, `/Volumes`, `/private`, `/tmp`. `/var/coast-data` **не** входит в список по умолчанию в macOS -- для новых путей лучше использовать `$HOME/coast-data/...` или добавить `/var/coast-data` в File Sharing. Форма `/var/lib/docker/volumes/<name>/_data` *не* является путём хоста -- Docker разрешает её внутри собственной VM -- поэтому она работает без записи в File Sharing.
- **WSL2.** Предпочитайте пути, нативные для WSL (`~`, `/mnt/wsl/...`). `/mnt/c/...` работает, но медленно из-за протокола 9P, который связывает файловую систему хоста Windows.
- **Linux.** Никаких подводных камней.

## Жизненный цикл

- `coast ssg rm` -- удаляет внешний контейнер DinD SSG. **Содержимое томов не затрагивается**, содержимое host bind-mount не затрагивается, keystore не затрагивается. Всё остальное, что использует тот же том Docker, продолжает работать.
- `coast ssg rm --with-data` -- удаляет тома, которые находятся **внутри вложенного демона Docker SSG** (форма `name:path` без начального слеша). Host bind mounts и внешние тома Docker по-прежнему не затрагиваются -- Coast ими не владеет.
- `coast ssg build` -- никогда не трогает тома. Только записывает манифест и (когда объявлен `[secrets]`) строки keystore.
- `coast ssg run` / `start` / `restart` -- создаёт каталоги host bind-mount, если они не существуют (со стандартным владельцем -- см. [Права доступа](#права-доступа)).

## См. также

- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) -- полная TOML-схема, включая синтаксис томов
- [Volume Topology](../concepts_and_terminology/VOLUMES.md) -- стратегии общих, изолированных и инициализируемых из снимков томов для сервисов вне SSG
- [Building](BUILDING.md) -- откуда берётся манифест
- [Lifecycle](LIFECYCLE.md) -- когда тома создаются, останавливаются и удаляются
- [Secrets](SECRETS.md) -- секреты, внедряемые как файлы, попадают в `~/.coast/ssg/runs/<project>/secrets/<basename>` и монтируются bind-монтированием во внутренние сервисы в режиме только чтения
