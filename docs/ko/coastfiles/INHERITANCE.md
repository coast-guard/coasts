# 상속, 타입, 그리고 컴포지션

Coastfile은 상속(`extends`), 프래그먼트 컴포지션(`includes`), 항목 제거(`[unset]`), 그리고 컴포즈 레벨 스트리핑(`[omit]`)을 지원합니다. 이를 함께 사용하면 기본 구성을 한 번만 정의하고, 설정을 중복하지 않으면서 서로 다른 워크플로우(테스트 러너, 경량 프런트엔드, 스냅샷 시드 스택 등)를 위한 가벼운 변형을 만들 수 있습니다.

타입이 지정된 Coastfile이 빌드 시스템에 어떻게 들어맞는지에 대한 상위 수준 개요는 [Coastfile Types](../concepts_and_terminology/COASTFILE_TYPES.md) 및 [Builds](../concepts_and_terminology/BUILDS.md)를 참고하세요.

## Coastfile 타입

기본 Coastfile의 이름은 항상 `Coastfile`입니다. 타입 변형은 `Coastfile.{type}` 네이밍 패턴을 사용합니다:

- `Coastfile` — 기본 타입
- `Coastfile.light` — 타입 `light`
- `Coastfile.snap` — 타입 `snap`
- `Coastfile.ci.minimal` — 타입 `ci.minimal`

`Coastfile.default`라는 이름은 예약되어 있으며 허용되지 않습니다. 끝에 점이 붙는 형태(`Coastfile.`)도 유효하지 않습니다.

`--type`으로 타입 변형을 빌드하고 실행합니다:

```
coast build --type light
coast run test-1 --type light
```

각 타입은 서로 독립적인 빌드 풀을 가집니다. `--type light` 빌드는 기본 빌드에 영향을 주지 않습니다.

## `extends`

타입이 지정된 Coastfile은 `[coast]` 섹션에서 `extends`를 사용해 부모로부터 상속할 수 있습니다. 부모가 먼저 완전히 파싱된 뒤, 자식의 값이 그 위에 레이어링됩니다.

```toml
[coast]
extends = "Coastfile"
```

값은 자식 디렉터리를 기준으로 해석되는 부모 Coastfile에 대한 상대 경로입니다. 체인도 지원됩니다 — 자식은 조부모를 상속하는 부모를 다시 상속할 수 있습니다:

```
Coastfile                    (base)
  └─ Coastfile.light         (extends Coastfile)
       └─ Coastfile.chain    (extends Coastfile.light)
```

순환 체인(A extends B extends A, 또는 A extends A)은 감지되어 거부됩니다.

### 병합 규칙

자식이 부모를 extends할 때:

- **스칼라 필드** (`name`, `runtime`, `compose`, `root`, `worktree_dir`, `autostart`, `primary_port`) — 자식 값이 있으면 자식 값이 우선하며, 없으면 부모로부터 상속됩니다.
- **맵** (`[ports]`, `[egress]`) — 키 기준으로 병합됩니다. 자식 키는 같은 이름의 부모 키를 덮어쓰며, 부모에만 있는 키는 유지됩니다.
- **이름이 있는 섹션** (`[secrets.*]`, `[volumes.*]`, `[shared_services.*]`, `[mcp.*]`, `[mcp_clients.*]`, `[services.*]`) — 이름 기준으로 병합됩니다. 같은 이름의 자식 엔트리는 부모 엔트리를 완전히 대체하며, 새로운 이름은 추가됩니다.
- **`[coast.setup]`**:
  - `packages` — 중복 제거된 합집합(자식이 새 패키지를 추가하고, 부모 패키지는 유지)
  - `run` — 자식 명령이 부모 명령 뒤에 추가(append)됩니다
  - `files` — `path` 기준으로 병합(같은 path = 자식 엔트리가 부모 엔트리를 대체)
- **`[inject]`** — `env` 및 `files` 리스트가 연결(concatenate)됩니다.
- **`[omit]`** — `services` 및 `volumes` 리스트가 연결(concatenate)됩니다.
- **`[assign]`** — 자식에 존재하면 전체가 대체됩니다(필드 단위 병합 아님).
- **`[agent_shell]`** — 자식에 존재하면 전체가 대체됩니다.

### 프로젝트 이름 상속

자식이 `name`을 설정하지 않으면 부모의 이름을 상속합니다. 이는 타입 변형에 대해 정상적인 동작입니다 — 같은 프로젝트의 변형이기 때문입니다:

```toml
# Coastfile
[coast]
name = "my-app"
```

```toml
# Coastfile.light — name "my-app"을 상속
[coast]
extends = "Coastfile"
autostart = false
```

변형을 별도 프로젝트로 보이게 하고 싶다면 자식에서 `name`을 오버라이드할 수 있습니다:

```toml
[coast]
extends = "Coastfile"
name = "my-app-light"
```

## `includes`

`includes` 필드는 파일 자체의 값이 적용되기 전에 하나 이상의 TOML 프래그먼트 파일을 Coastfile에 병합합니다. 이는 공유 구성(예: 시크릿 세트나 MCP 서버)을 재사용 가능한 프래그먼트로 분리할 때 유용합니다.

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]
```

포함되는 프래그먼트는 Coastfile과 동일한 섹션 구조를 가진 TOML 파일입니다. `[coast]` 섹션(비어 있어도 됨)을 포함해야 하지만, 프래그먼트 자체에서 `extends`나 `includes`를 사용할 수는 없습니다.

```toml
# extra-secrets.toml
[coast]

[secrets.mongo_uri]
extractor = "env"
var = "MONGO_URI"
inject = "env:MONGO_URI"
```

`extends`와 `includes`가 모두 있을 때의 병합 순서:

1. (재귀적으로) `extends`를 통해 부모를 파싱
2. 포함된 각 프래그먼트를 순서대로 병합
3. 파일 자체의 값을 적용(모든 것보다 우선)

## `[unset]`

모든 병합이 완료된 뒤 해석된 설정에서 이름이 있는 항목을 제거합니다. 이는 자식이 부모로부터 상속받은 무언가를, 섹션 전체를 재정의하지 않고도 제거하는 방법입니다.

```toml
[unset]
secrets = ["db_password"]
shared_services = ["postgres", "redis"]
ports = ["postgres", "redis"]
```

지원되는 필드:

- `secrets` — 제거할 시크릿 이름 목록
- `ports` — 제거할 포트 이름 목록
- `shared_services` — 제거할 공유 서비스 이름 목록
- `volumes` — 제거할 볼륨 이름 목록
- `mcp` — 제거할 MCP 서버 이름 목록
- `mcp_clients` — 제거할 MCP 클라이언트 이름 목록
- `egress` — 제거할 이그레스(egress) 이름 목록
- `services` — 제거할 베어 서비스 이름 목록

`[unset]`은 전체 extends + includes 병합 체인이 해석된 뒤 적용됩니다. 최종 병합 결과에서 이름으로 항목을 제거합니다.

## `[omit]`

Coast 내부에서 실행되는 Docker Compose 스택에서 컴포즈 서비스와 볼륨을 제거(스트립)합니다. Coastfile 레벨 구성을 제거하는 `[unset]`과 달리, `[omit]`은 DinD 컨테이너 내부에서 `docker compose up`을 실행할 때 특정 서비스나 볼륨을 제외하도록 Coast에 지시합니다.

```toml
[omit]
services = ["monitoring", "debug-tools", "nginx-proxy"]
volumes = ["keycloak-db-data"]
```

- **`services`** — `docker compose up`에서 제외할 컴포즈 서비스 이름
- **`volumes`** — 제외할 컴포즈 볼륨 이름

이는 `docker-compose.yml`이 모든 Coast 변형에서 필요하지 않은 서비스(모니터링 스택, 리버스 프록시, 관리자 도구)를 정의할 때 유용합니다. 여러 컴포즈 파일을 유지하는 대신, 단일 컴포즈 파일을 사용하고 변형별로 필요 없는 것을 제거합니다.

자식이 부모를 extends할 때, `[omit]` 리스트는 연결(concatenate)됩니다 — 자식이 부모의 omit 목록에 추가합니다.

## Examples

### Lightweight test variant

기본 Coastfile을 extends하고, autostart를 비활성화하며, 공유 서비스를 제거하고, 데이터베이스를 인스턴스별로 격리하여 실행합니다:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "backend", "postgres", "redis"]
shared_services = ["postgres", "redis", "mongodb"]

[omit]
services = ["redis", "backend", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[assign]
default = "none"
[assign.services]
backend-test = "rebuild"
migrations = "rebuild"
```

### Snapshot-seeded variant

기본의 공유 서비스를 제거하고 스냅샷으로 시드된 격리 볼륨으로 대체합니다:

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis", "mongodb"]

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

### Typed variant with extra shared services and includes

기본을 extends하고 MongoDB를 추가하며, 프래그먼트에서 추가 시크릿을 가져옵니다:

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]

[ports]
mongodb = 37017

[shared_services.mongodb]
image = "mongo:7"
ports = [27017]
env = { MONGO_INITDB_ROOT_USERNAME = "dev", MONGO_INITDB_ROOT_PASSWORD = "dev" }

[omit]
services = ["debug-tools"]
```

### Multi-level inheritance chain

3단계 깊이: base -> light -> chain.

```toml
# Coastfile.chain
[coast]
extends = "Coastfile.light"

[coast.setup]
run = ["echo 'chain setup appended'"]

[ports]
debug = 39999
```

해석된 구성은 기본 `Coastfile`로 시작해 그 위에 `Coastfile.light`를 병합하고, 다시 그 위에 `Coastfile.chain`을 병합합니다. 세 레벨의 Setup `run` 명령은 순서대로 모두 연결(concatenate)됩니다. Setup `packages`는 모든 레벨에 걸쳐 중복 제거됩니다.

### Omitting services from a large compose stack

개발에 필요하지 않은 `docker-compose.yml`의 서비스를 제거합니다:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[omit]
services = ["backend-debug", "backend-debug-test", "asynqmon", "postgres-keycloak", "keycloak", "redash-db-init", "redash-init", "redash", "redash-scheduler", "redash-worker", "langfuse-db-init", "langfuse", "nginx-proxy"]
volumes = ["keycloak-db-data"]

[ports]
web = 3000
backend = 8080
```
