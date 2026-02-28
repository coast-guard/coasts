# 공유 서비스

`[shared_services.*]` 섹션은 개별 Coast 컨테이너 내부가 아니라 호스트 Docker 데몬에서 실행되는 인프라 서비스(데이터베이스, 캐시, 메시지 브로커)를 정의합니다. 여러 Coast 인스턴스가 브리지 네트워크를 통해 동일한 공유 서비스에 연결합니다.

런타임에서 공유 서비스가 동작하는 방식, 라이프사이클 관리, 그리고 문제 해결에 대해서는 [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md)를 참고하세요.

## 공유 서비스 정의하기

각 공유 서비스는 `[shared_services]` 아래의 이름이 지정된 TOML 섹션입니다. `image` 필드는 필수이며, 그 외는 모두 선택 사항입니다.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image` (필수)

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

데이터 영속화를 위한 Docker 볼륨 바인드 문자열입니다. 이는 Coast가 관리하는 볼륨이 아니라 호스트 수준의 Docker 볼륨입니다.

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

## 라이프사이클

공유 서비스는 이를 참조하는 첫 번째 Coast 인스턴스가 실행될 때 자동으로 시작됩니다. 또한 `coast stop` 및 `coast rm` 이후에도 계속 실행됩니다 — 인스턴스를 제거해도 공유 서비스 데이터에는 영향을 주지 않습니다. 오직 `coast shared rm`만이 공유 서비스를 중지하고 제거합니다.

`auto_create_db`로 생성된 인스턴스별 데이터베이스 역시 인스턴스 삭제 후에도 유지됩니다. 이를 명시적으로 제거하려면 `coast shared db drop`을 사용하세요.

## 공유 서비스 vs 볼륨을 언제 사용할까

여러 Coast 인스턴스가 동일한 데이터베이스 서버와 통신해야 할 때 공유 서비스를 사용하세요(예: 각 인스턴스에 자체 데이터베이스가 할당되는 공유 Postgres). compose 내부 서비스의 데이터를 공유하거나 격리하는 방식을 제어하고 싶다면 [volume strategies](VOLUMES.md)를 사용하세요.

## 예시

### Postgres, Redis, 그리고 MongoDB

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

### 최소 구성 공유 Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

### 자동 생성 데이터베이스가 있는 공유 서비스

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```
