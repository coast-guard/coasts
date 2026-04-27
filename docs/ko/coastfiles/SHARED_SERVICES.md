# Shared Services

`[shared_services.*]` 섹션은 Coast 프로젝트가 소비하는 인프라 서비스 -- 데이터베이스, 캐시, 메시지 브로커 -- 를 정의합니다. 두 가지 방식이 있습니다:

- **인라인** -- 소비자 Coastfile에 `image`, `ports`, `env`, `volumes`를 직접 선언합니다. Coast는 호스트 측 컨테이너를 시작하고 소비자 앱의 트래픽을 그쪽으로 라우팅합니다. 소비자 인스턴스가 하나뿐인 단독 프로젝트이거나, 아주 가벼운 서비스에 가장 적합합니다.
- **공유 서비스 그룹에서 (`from_group = true`)** -- 서비스는 프로젝트의 [Shared Service Group](../shared_service_groups/README.md)에 존재합니다 (`Coastfile.shared_service_groups`에 선언되는 별도의 DinD 컨테이너). 소비자 Coastfile은 단지 사용을 선택만 합니다. 시크릿 추출, canonical 포트로의 호스트 측 checkout, 또는 이 호스트에서 각각 동일한 canonical 포트를 필요로 하는 여러 Coast 프로젝트를 실행할 때 가장 적합합니다 (SSG는 호스트의 5432를 바인딩하지 않고도 내부 `:5432`에서 Postgres를 유지하므로 두 프로젝트가 공존할 수 있습니다).

이 페이지의 두 부분은 각각의 방식을 차례대로 설명합니다.

공유 서비스의 런타임 동작 방식, 라이프사이클 관리, 문제 해결에 대해서는 [Shared Services (concept)](../concepts_and_terminology/SHARED_SERVICES.md)를 참고하세요.

---

## Inline shared services

각 인라인 서비스는 `[shared_services]` 아래의 이름 있는 TOML 섹션입니다. `image` 필드는 필수이며 나머지는 모두 선택 사항입니다.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image` (required)

호스트 데몬에서 실행할 Docker 이미지입니다.

### `ports`

서비스가 노출하는 포트 목록입니다. Coast는 컨테이너 포트만 지정하는 형식과
Docker Compose 스타일의 `"HOST:CONTAINER"` 매핑 형식을 모두 허용합니다.

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- `6379` 같은 정수값 하나는 `"6379:6379"`의 축약형입니다.
- `"5433:5432"` 같은 매핑 문자열은 공유 서비스를 호스트 포트
  `5433`에 게시하면서, Coast 내부에서는 `service-name:5432`로 계속 접근 가능하게 합니다.
- 호스트 포트와 컨테이너 포트는 둘 다 0이 아니어야 합니다.

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

`true`이면, Coast는 각 Coast 인스턴스마다 공유 서비스 내부에 인스턴스별 데이터베이스를 자동 생성합니다. 기본값은 `false`입니다.

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

### Lifecycle

인라인 공유 서비스는 이를 참조하는 첫 번째 Coast 인스턴스가 실행될 때 자동으로 시작됩니다. 또한 `coast stop` 및 `coast rm` 이후에도 계속 실행됩니다 -- 인스턴스를 제거해도 공유 서비스 데이터에는 영향이 없습니다. 오직 `coast shared rm`만 서비스를 중지하고 제거합니다.

`auto_create_db`로 생성된 인스턴스별 데이터베이스도 인스턴스 삭제 후에도 유지됩니다. 서비스와 그 데이터를 완전히 제거하려면 `coast shared-services rm`을 사용하세요.

### Inline examples

#### Postgres, Redis, and MongoDB

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

#### Minimal shared Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### Host/container mapped Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = ["5433:5432"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### Auto-created databases

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

---

## Shared services from a Shared Service Group

더 구조화된 공유 인프라 설정 -- 여러 워크트리, 호스트 측 checkout, SSG 네이티브 시크릿, SSG 재빌드 전반에 걸친 가상 포트 -- 을 원하는 프로젝트의 경우, [`Coastfile.shared_service_groups`](SHARED_SERVICE_GROUPS.md)에 서비스를 한 번 선언하고 소비자 Coastfile에서는 `from_group = true`로 참조하세요:

```toml
[shared_services.postgres]
from_group = true

# Optional per-consumer overrides:
inject = "env:DATABASE_URL"
# auto_create_db = false    # overrides the SSG service's default
```

TOML 키 (이 예시에서는 `postgres`) 는 프로젝트의 `Coastfile.shared_service_groups`에 선언된 서비스와 일치해야 합니다. 여기서 참조되는 SSG는 **항상 소비자 프로젝트 자신의 SSG**입니다 (`<project>-ssg`라는 이름이며, `<project>`는 소비자의 `[coast].name`입니다).

### Forbidden fields with `from_group = true`

다음 필드들은 SSG가 단일 진실 공급원이므로 파싱 시점에 거부됩니다:

- `image`
- `ports`
- `env`
- `volumes`

이 중 어떤 것이든 `from_group = true`와 함께 있으면 다음이 발생합니다:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports, env, volumes.
```

### Allowed per-consumer overrides

- `inject` -- 연결 문자열을 노출할 환경 변수 또는 파일 경로입니다. 서로 다른 소비자 Coastfile은 동일한 SSG Postgres를 서로 다른 환경 변수 이름으로 노출할 수 있습니다.
- `auto_create_db` -- `coast run` 시점에 Coast가 이 서비스 내부에 인스턴스별 데이터베이스를 생성할지 여부입니다. SSG 서비스 자체의 `auto_create_db` 값을 덮어씁니다.

### Missing-service error

프로젝트의 `Coastfile.shared_service_groups`에 선언되지 않은 이름을 참조하면, `coast build`가 실패합니다:

```text
error: shared service 'postgres' has from_group = true but no service named 'postgres' is declared in Coastfile.shared_service_groups for project 'my-app'.
```

### When to choose `from_group` over inline

| Need | Inline | `from_group` |
|---|---|---|
| 이 호스트에서 단일 Coast 프로젝트만 실행하고, 시크릿이 필요 없음 | 둘 다 가능; 인라인이 더 단순함 | 가능 |
| 하나의 Postgres를 공유하는 **동일한** 프로젝트의 여러 워크트리 / 소비자 인스턴스 | 가능 (형제 인스턴스들이 하나의 호스트 컨테이너를 공유) | 가능 |
| 이 호스트에서 각각 동일한 canonical 포트를 선언하는 **서로 다른 두 Coast 프로젝트** (예: 둘 다 5432의 Postgres를 원함) | 호스트 포트에서 충돌; 동시에 둘 다 실행할 수 없음 | 필요함 (각 프로젝트의 SSG가 호스트 5432를 바인딩하지 않고 자체 내부 Postgres를 소유) |
| `coast ssg checkout`을 통한 호스트 측 `psql localhost:5432`를 원함 | -- | 필요함 |
| 서비스에 대해 빌드 시점 시크릿 추출이 필요함 (`POSTGRES_PASSWORD`를 키체인 등에서 가져오기) | -- | 필요함 (참조: [SSG Secrets](../shared_service_groups/SECRETS.md)) |
| 재빌드 전반에 걸친 안정적인 소비자 라우팅(가상 포트) | -- | 필요함 (참조: [SSG Routing](../shared_service_groups/ROUTING.md)) |

전체 SSG 아키텍처에 대해서는 [Shared Service Groups](../shared_service_groups/README.md)를 참고하세요. 자동 시작, 드리프트 감지, 원격 소비자를 포함한 소비자 측 경험에 대해서는 [Consuming](../shared_service_groups/CONSUMING.md)을 참고하세요.

---

## See Also

- [Shared Services (concept)](../concepts_and_terminology/SHARED_SERVICES.md) -- 두 방식 모두에 대한 런타임 아키텍처
- [Shared Service Groups](../shared_service_groups/README.md) -- SSG 개념 개요
- [Coastfile: Shared Service Groups](SHARED_SERVICE_GROUPS.md) -- SSG 측 Coastfile 스키마
- [Consuming an SSG](../shared_service_groups/CONSUMING.md) -- `from_group = true` 의미론에 대한 자세한 안내
