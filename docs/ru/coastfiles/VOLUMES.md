# Томa

Разделы `[volumes.*]` управляют тем, как именованные Docker-тома обрабатываются между экземплярами Coast. Каждый том настраивается со стратегией, которая определяет, будут ли экземпляры совместно использовать данные или получат собственную независимую копию.

Чтобы увидеть общую картину изоляции данных в Coast — включая общие сервисы как альтернативу — см. [Volumes](../concepts_and_terminology/VOLUMES.md).

## Определение тома

Каждый том — это именованный раздел TOML внутри `[volumes]`. Требуются три поля:

- **`strategy`** — `"isolated"` или `"shared"`
- **`service`** — имя compose-сервиса, который использует этот том
- **`mount`** — путь монтирования тома внутри контейнера

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"
```

## Стратегии

### `isolated`

Каждый экземпляр Coast получает свой собственный независимый том. Данные не разделяются между экземплярами. Томa создаются при `coast run` и удаляются при `coast rm`.

```toml
[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"
```

Это правильный выбор для большинства томов баз данных — каждый экземпляр получает «чистый лист» и может свободно изменять данные, не влияя на другие экземпляры.

### `shared`

Все экземпляры Coast используют один общий Docker-том. Любые данные, записанные одним экземпляром, видны всем остальным.

```toml
[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

Общие томa никогда не удаляются командой `coast rm`. Они сохраняются, пока вы не удалите их вручную.

Coast выводит предупреждение на этапе сборки, если вы используете `shared` для тома, подключённого к сервису типа базы данных. Совместное использование одного тома базы данных несколькими параллельными экземплярами может привести к повреждению данных. Если вам нужны общие базы данных, используйте вместо этого [shared services](SHARED_SERVICES.md).

Хорошие варианты использования общих томов: кэши зависимостей (Go modules, npm cache, pip cache), кэши артефактов сборки и другие данные, где параллельные записи безопасны или маловероятны.

## Наполнение из снапшота

Изолированные томa можно заполнить данными из существующего Docker-тома во время создания экземпляра с помощью `snapshot_source`. Данные исходного тома копируются в новый изолированный том, который затем независимо расходится.

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "db"
mount = "/var/lib/postgresql/data"
```

`snapshot_source` допустим только при `strategy = "isolated"`. Указание его для общего тома является ошибкой.

Это полезно, когда вы хотите, чтобы каждый экземпляр Coast начинал с реалистичного набора данных, скопированного из вашей хостовой базы данных разработки, но при этом вы хотите, чтобы экземпляры могли свободно изменять эти данные, не влияя на источник или друг на друга.

## Примеры

### Изолированные базы данных, общий кэш зависимостей

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"

[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

### Полный стек с наполнением из снапшота

Каждый экземпляр начинает с копии существующих томов баз данных вашего хоста, а затем независимо расходится.

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "infra_redis_data"
service = "redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
snapshot_source = "infra_mongodb_data"
service = "mongodb"
mount = "/data/db"
```

### Запуск тестов с чистыми базами данных для каждого экземпляра

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
service = "mongodb"
mount = "/data/db"
```
