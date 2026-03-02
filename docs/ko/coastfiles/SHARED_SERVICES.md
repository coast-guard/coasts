# Shared Services

`[shared_services.*]` 섹션은 개별 Coast 컨테이너 내부가 아니라 호스트 Docker 데몬에서 실행되는 인프라 서비스(데이터베이스, 캐시, 메시지 브로커)를 정의합니다. 여러 Coast 인스턴스가 브리지 네트워크를 통해 동일한 공유 서비스에 연결합니다.

런타임에서 공유 서비스가 동작하는 방식, 라이프사이클 관리, 문제 해결에 대해서는 [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md)를 참고하세요.

## Defining a shared service

각 공유 서비스는 `[shared_services]` 아래의 이름이 있는 TOML 섹션입니다. `image` 필드는 필수이며, 그 외는 모두 선택 사항입니다.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image` (required)

호스트 데몬에서 실행할 Docker 이미지입니다.

### `ports`

서비스가 노출하는 포트 목록입니다. 공유 서비스와 Coast 인스턴스 간의 브리지 네트워크 라우팅에 사용됩니다.

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

포트 값은 0이 아니어야 합니다.

### `volumes`

데이터 영속성을 위한 Docker 볼륨 바인드 문자열입니다. 이는 Coast가 관리하는 볼륨이 아니라 호스트 수준의 Docker 볼륨입니다.

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
```

### `env`

서비스 컨테이너에 전달되는 환경 변수입니다.

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_DB = "mydb" }
```

### `auto_create_db`

`true`인 경우, Coast는 각 Coast 인스턴스마다 공유 서비스 내부에 인스턴스별 데이터베이스를 자동으로 생성합니다. 기본값은 `false`입니다.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
auto_create_db = true
```

### `inject`

공유 서비스 연결 정보를 환경 변수 또는 파일로 Coast 인스턴스에 주입합니다. [secrets](SECRETS.md)와 동일한 `env:NAME` 또는 `file:/path` 형식을 사용합니다.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
inject = "env:DATABASE_URL"
```

## Lifecycle

공유 서비스는 이를 참조하는 첫 번째 Coast 인스턴스가 실행될 때 자동으로 시작됩니다. 또한 `coast stop` 및 `coast rm` 이후에도 계속 실행됩니다 — 인스턴스를 제거해도 공유 서비스 데이터에는 영향이 없습니다. 오직 `coast shared rm`만 공유 서비스를 중지하고 제거합니다.

`auto_create_db`로 생성된 인스턴스별 데이터베이스도 인스턴스 삭제 후에도 유지됩니다. 서비스와 그 데이터를 완전히 제거하려면 `coast shared-services rm`을 사용하세요.

## When to use shared services vs volumes

여러 Coast 인스턴스가 동일한 데이터베이스 서버와 통신해야 할 때 공유 서비스를 사용하세요(예: 각 인스턴스가 자체 데이터베이스를 받는 공유 Postgres). compose 내부 서비스의 데이터를 공유하거나 격리하는 방식을 제어하려면 [volume strategies](VOLUMES.md)를 사용하세요.

## Examples

### Postgres, Redis, and MongoDB

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_MULTIPLE_DATABASES = "dev_db,test_db" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis_data:/data"]

[shared_services.mongodb]
image = "mongo:latest"
ports = [27017]
volumes = ["infra_mongodb_data:/data/db"]
env = { MONGO_INITDB_ROOT_USERNAME = "myapp", MONGO_INITDB_ROOT_PASSWORD = "myapp_pass" }
```

### Minimal shared Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

### Shared services with auto-created databases

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```
