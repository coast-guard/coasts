# Группы общих сервисов

Группа общих сервисов (SSG) — это контейнер Docker-in-Docker, который запускает инфраструктурные сервисы вашего проекта — Postgres, Redis, MongoDB, всё, что в противном случае вы бы поместили в `[shared_services]` — в одном месте, отдельно от экземпляров [Coast](../concepts_and_terminology/COASTS.md), которые его используют. Каждый проект Coast получает собственный SSG с именем `<project>-ssg`, объявленный в `Coastfile.shared_service_groups`, находящемся рядом с `Coastfile` проекта.

Каждый экземпляр-потребитель (`dev-1`, `dev-2`, ...) подключается к SSG своего проекта через стабильные виртуальные порты, поэтому пересборки SSG не приводят к изменениям у потребителей. Внутри каждого Coast контракт остаётся неизменным: `postgres:5432` указывает на ваш общий Postgres, и код приложения не знает, что происходит что-то особенное.

## Зачем нужен SSG

Изначальный шаблон [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) запускает один инфраструктурный контейнер в Docker daemon хоста и разделяет его между всеми экземплярами-потребителями в проекте. Это прекрасно работает для одного проекта. Проблемы начинаются, когда у вас есть **два разных проекта**, и каждый из них объявляет Postgres на `5432`: оба проекта пытаются привязать один и тот же порт хоста, и второй завершается с ошибкой.

```text
Without an SSG (cross-project host-port collision):

Host Docker daemon
+-- cg-coasts-postgres            (project "cg" binds host :5432)
+-- filemap-coasts-postgres       (project "filemap" tries :5432 -- FAILS)
+-- cg-coasts-dev-1               --> cg-coasts-postgres
+-- cg-coasts-dev-2               --> cg-coasts-postgres   (siblings share fine)
```

SSG решают эту проблему, поднимая инфраструктуру каждого проекта в собственный DinD. Postgres по-прежнему слушает на каноническом `:5432` — но внутри SSG, а не на хосте. Контейнер SSG публикуется на произвольном динамическом порту хоста, а управляемый демоном virtual-port socat (в диапазоне `42000-43000`) перенаправляет к нему трафик потребителей. Два проекта могут иметь Postgres на каноническом 5432, потому что ни один из них не привязывает порт хоста 5432:

```text
With an SSG (per project, no cross-project collision):

Host Docker daemon
+-- cg-ssg                        (project "cg" -- DinD)
|     +-- postgres                (inner :5432, host dyn 54201, vport 42000)
|     +-- redis                   (inner :6379, host dyn 54202, vport 42001)
+-- filemap-ssg                   (project "filemap" -- DinD, no collision)
|     +-- postgres                (inner :5432, host dyn 54250, vport 42002)
|     +-- redis                   (inner :6379, host dyn 54251, vport 42003)
+-- cg-coasts-dev-1               --> hg-internal:42000 --> cg-ssg postgres
+-- cg-coasts-dev-2               --> hg-internal:42000 --> cg-ssg postgres
+-- filemap-coasts-dev-1          --> hg-internal:42002 --> filemap-ssg postgres
```

SSG каждого проекта владеет собственными данными, собственными версиями образов и собственными секретами. Они никогда не разделяют состояние, не конкурируют за порты и не видят данные друг друга. Внутри каждого Coast-потребителя контракт остаётся неизменным: код приложения подключается к `postgres:5432` и получает Postgres своего проекта — остальное делает слой маршрутизации (см. [Routing](ROUTING.md)).

## Быстрый старт

`Coastfile.shared_service_groups` находится рядом с `Coastfile` проекта. Имя проекта берётся из `[coast].name` в обычном Coastfile — повторно указывать его не нужно.

```toml
# Coastfile.shared_service_groups
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["pg_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_DB = "app_dev" }
auto_create_db = true

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]

# Optional: extract secrets from your environment, keychain, or 1Password
# at build time and inject them into the SSG at run time. See SECRETS.md.
[secrets.pg_password]
extractor = "env"
inject = "env:POSTGRES_PASSWORD"
var = "MY_PG_PASSWORD"
```

Соберите и запустите его:

```bash
coast ssg build       # parse, pull images, extract secrets, write artifact
coast ssg run         # start <project>-ssg, materialize secrets, compose up
coast ssg ps          # show service status
```

Укажите экземпляру Coast-потребителя использовать его:

```toml
# Coastfile in the same project
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true
inject = "env:DATABASE_URL"

[shared_services.redis]
from_group = true
```

Затем выполните `coast build && coast run dev-1`. SSG будет автоматически запущен, если он ещё не работает. Внутри контейнера приложения `dev-1` `postgres:5432` указывает на Postgres SSG, а `$DATABASE_URL` устанавливается в каноническую строку подключения.

## Справочник

| Страница | Что она охватывает |
|---|---|
| [Building](BUILDING.md) | `coast ssg build` от начала до конца, структура артефактов для каждого проекта, извлечение секретов, правила обнаружения `Coastfile.shared_service_groups` и как закрепить проект за конкретной сборкой |
| [Lifecycle](LIFECYCLE.md) | `run` / `start` / `stop` / `restart` / `rm` / `ps` / `logs` / `exec`, контейнер `<project>-ssg` для каждого проекта, автоматический запуск при `coast run` и `coast ssg ls` для просмотра списка между проектами |
| [Routing](ROUTING.md) | Канонические / динамические / виртуальные порты, слой host socat, полная цепочка переходов от приложения до внутреннего сервиса и симметричные туннели для удалённых потребителей |
| [Volumes](VOLUMES.md) | Bind mounts хоста, симметричные пути, внутренние именованные тома, права доступа, команда `coast ssg doctor` и перенос существующего host volume в SSG |
| [Consuming](CONSUMING.md) | `from_group = true`, разрешённые и запрещённые поля, обнаружение конфликтов, `auto_create_db`, `inject` и удалённые потребители |
| [Secrets](SECRETS.md) | `[secrets.<name>]` в SSG Coastfile, конвейер извлечения на этапе сборки, инъекция во время выполнения через `compose.override.yml` и команда `coast ssg secrets clear` |
| [Checkout](CHECKOUT.md) | `coast ssg checkout` / `uncheckout` для привязки канонических портов SSG на хосте, чтобы любое приложение на вашем хосте (psql, redis-cli, IDE) могло к ним обращаться |
| [CLI](CLI.md) | Краткое описание в одну строку для каждой подкоманды `coast ssg` |

## См. также

- [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) -- встроенный-в-экземпляр шаблон, который SSG обобщает
- [Shared Services Coastfile reference](../coastfiles/SHARED_SERVICES.md) -- TOML-синтаксис на стороне потребителя, включая `from_group`
- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) -- полная схема для `Coastfile.shared_service_groups`
- [Ports](../concepts_and_terminology/PORTS.md) -- канонические и динамические порты
