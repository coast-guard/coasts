# Coastfile.shared_service_groups

`Coastfile.shared_service_groups`는 프로젝트의 Shared Service Group(SSG)이 실행할 서비스를 선언하는 타입드 Coastfile입니다. 일반 `Coastfile` 옆에 위치하며, 프로젝트 이름은 그 형제 파일의 `[coast].name`에서 가져오므로 여기서 다시 반복하지 않습니다. 각 프로젝트는 정확히 하나의 이러한 파일을 가지며(작업 트리 내에서), `<project>-ssg` 컨테이너가 이 파일이 선언한 서비스를 실행합니다. 같은 프로젝트의 다른 consumer Coastfile은 `[shared_services.<name>] from_group = true`로 이 서비스를 참조할 수 있습니다.

개념, 라이프사이클, 볼륨, 시크릿, consumer 연결에 대해서는 [Shared Service Groups documentation](../shared_service_groups/README.md)를 참조하세요.

## Discovery

`coast ssg build`는 `coast build`와 동일한 규칙으로 파일을 찾습니다:

- 기본값: 현재 작업 디렉터리에서 `Coastfile.shared_service_groups` 또는 `Coastfile.shared_service_groups.toml`을 찾습니다. 두 형식은 동등하며, 둘 다 존재할 경우 `.toml` 변형이 우선합니다.
- `-f <path>` / `--file <path>`는 임의의 파일을 가리킵니다.
- `--working-dir <dir>`는 프로젝트 루트와 Coastfile 위치를 분리합니다.
- `--config '<toml>'`는 스크립트된 흐름을 위한 인라인 TOML을 받습니다.

## Accepted Sections

`[ssg]`, `[shared_services.<name>]`, `[secrets.<name>]`, `[unset]`만 허용됩니다. 그 외의 최상위 키(`[coast]`, `[ports]`, `[services]`, `[volumes]`, `[assign]`, `[omit]`, `[inject]`, ...)는 파싱 시 거부됩니다.

합성을 위해 `[ssg] extends = "<path>"`와 `[ssg] includes = ["<path>", ...]`가 지원됩니다. 아래의 [Inheritance](#inheritance)를 참조하세요.

## `[ssg]`

최상위 SSG 구성입니다.

```toml
[ssg]
runtime = "dind"
```

### `runtime` (optional)

외부 SSG DinD를 위한 컨테이너 런타임입니다. 현재 지원되는 값은 `dind`뿐이며, 이 필드는 선택 사항이고 기본값은 `dind`입니다.

## `[shared_services.<name>]`

서비스당 하나의 블록입니다. TOML 키(`postgres`, `redis`, ...)가 consumer Coastfile이 참조하는 서비스 이름이 됩니다.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

### `image` (required)

SSG의 내부 Docker 데몬 안에서 실행할 Docker 이미지입니다. 호스트가 pull할 수 있는 공개 또는 비공개 이미지라면 모두 허용됩니다.

### `ports`

서비스가 수신 대기하는 컨테이너 포트입니다. **오직 정수만 허용됩니다.**

```toml
ports = [5432]
ports = [5432, 5433]
```

- `"HOST:CONTAINER"` 매핑(`"5432:5432"`)은 **거부됩니다**. SSG 호스트 게시 포트는 항상 동적이므로, 호스트 포트를 직접 선택하지 않습니다.
- 빈 배열(또는 필드를 완전히 생략하는 것)도 허용됩니다. 노출된 포트가 없는 사이드카도 괜찮습니다.

각 포트는 `coast ssg run` 시 외부 DinD에서 `PUBLISHED:CONTAINER` 매핑이 되며, 여기서 `PUBLISHED`는 동적으로 할당된 호스트 포트입니다. 안정적인 consumer 라우팅을 위해 프로젝트별 가상 포트도 별도로 할당됩니다 -- [Routing](../shared_service_groups/ROUTING.md)을 참조하세요.

### `env`

내부 서비스 컨테이너의 환경으로 그대로 전달되는 평면 문자열 맵입니다.

```toml
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "app" }
```

Env 값은 빌드 매니페스트에 **캡처되지 않습니다**. `coast build`의 안전성 원칙과 마찬가지로 키만 기록됩니다.

Coastfile에 하드코딩하고 싶지 않은 값(비밀번호, API 토큰)의 경우, 아래에 설명된 `[secrets.*]` 섹션을 사용하세요 -- 빌드 시점에 호스트에서 추출하고 실행 시점에 주입합니다.

### `volumes`

Docker-Compose 스타일 볼륨 문자열의 배열입니다. 각 항목은 다음 중 하나입니다:

```toml
volumes = [
    "/var/coast-data/postgres:/var/lib/postgresql/data",   # host bind mount
    "pg_wal:/var/lib/postgresql/wal",                       # inner named volume
]
```

**Host bind mount** -- 소스가 `/`로 시작합니다. 바이트는 실제 호스트 파일시스템에 저장됩니다. 외부 DinD와 내부 서비스는 **같은 호스트 경로 문자열**을 바인드합니다. [Volumes -> Symmetric-Path Plan](../shared_service_groups/VOLUMES.md#the-symmetric-path-plan)을 참조하세요.

**Inner named volume** -- 소스가 Docker 볼륨 이름입니다(`/` 없음). 이 볼륨은 SSG의 내부 Docker 데몬 안에 존재합니다. SSG 재시작 간에 유지되며, 호스트에서는 내용을 알 수 없습니다.

파싱 시 거부되는 항목:

- 상대 경로 (`./data:/...`).
- `..` 구성 요소.
- 컨테이너 전용 볼륨(소스 없음).
- 단일 서비스 내 중복 대상.

### `auto_create_db`

`true`일 경우, 데몬은 실행되는 각 consumer Coast에 대해 이 서비스 내부에 `{instance}_{project}` 데이터베이스를 생성합니다. 인식된 데이터베이스 이미지(Postgres, MySQL)에만 적용됩니다. 기본값은 `false`입니다.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

consumer Coastfile은 프로젝트별로 이 값을 재정의할 수 있습니다 -- [Consuming -> auto_create_db](../shared_service_groups/CONSUMING.md#auto_create_db)를 참조하세요.

### `inject` (not allowed)

`inject`는 SSG 서비스 정의에서 **유효하지 않습니다**. Injection은 consumer 측의 관심사입니다(서로 다른 consumer Coastfile은 같은 SSG Postgres를 서로 다른 env-var 이름으로 노출하기를 원할 수 있습니다). consumer 측 `inject` 의미론은 [Coastfile: Shared Services](SHARED_SERVICES.md#inject)를 참조하세요.

## `[secrets.<name>]`

`Coastfile.shared_service_groups`의 `[secrets.*]` 블록은 `coast ssg build` 시점에 호스트 측 자격 증명을 추출하고, `coast ssg run` 시점에 이를 SSG의 내부 서비스에 주입합니다. 스키마는 일반 Coastfile의 `[secrets.*]`를 반영하며(필드 참조는 [Secrets](SECRETS.md) 참조), SSG 전용 동작은 [SSG Secrets](../shared_service_groups/SECRETS.md)에 문서화되어 있습니다.

```toml
[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"

[secrets.tls_cert]
extractor = "file"
path = "/Users/me/certs/dev.pem"
inject = "file:/etc/ssl/certs/server.pem"
```

동일한 extractor를 사용할 수 있습니다(`env`, `file`, `command`, `keychain`, 사용자 정의 `coast-extractor-<name>`). `inject` 지시어는 값이 SSG의 내부 서비스 컨테이너 안에서 env var로 들어갈지 파일로 들어갈지를 선택합니다.

기본적으로 SSG 네이티브 시크릿은 선언된 모든 `[shared_services.*]`에 주입됩니다. 일부에만 적용하려면 서비스 이름을 명시적으로 나열하세요:

```toml
[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"
services = ["postgres"]      # only mounted on the postgres service
```

추출된 시크릿 값은 `coast_image = "ssg:<project>"` 아래 `~/.coast/keystore.db`에 암호화되어 저장됩니다 -- 이는 일반 Coast keystore 항목과는 별도의 네임스페이스입니다. `coast ssg secrets clear` 명령을 포함한 전체 라이프사이클은 [SSG Secrets](../shared_service_groups/SECRETS.md)를 참조하세요.

## Inheritance

SSG Coastfile은 일반 Coastfile과 동일한 `extends` / `includes` / `[unset]` 메커니즘을 지원합니다. 공통된 사고 모델은 [Coastfile Inheritance](INHERITANCE.md)를 참조하세요. 이 섹션은 SSG 전용 형태를 문서화합니다.

### `[ssg] extends` -- 부모 Coastfile 가져오기

```toml
[ssg]
extends = "Coastfile.ssg-base"

[shared_services.postgres]
image = "postgres:17-alpine"
```

부모 파일은 자식의 부모 디렉터리를 기준으로 해석됩니다. `.toml` 우선 규칙이 적용됩니다(파서는 먼저 `Coastfile.ssg-base.toml`을 시도하고, 그다음 일반 `Coastfile.ssg-base`를 시도합니다). 절대 경로도 허용됩니다.

### `[ssg] includes` -- 프래그먼트 파일 병합

```toml
[ssg]
includes = ["dev-seed.toml", "extra-caches.toml"]

[shared_services.postgres]
image = "postgres:16-alpine"
```

프래그먼트는 포함하는 파일 자체보다 먼저, 순서대로 병합됩니다. 프래그먼트 경로는 포함하는 파일의 부모 디렉터리를 기준으로 해석됩니다(`.toml` 우선 규칙 없음 -- 프래그먼트는 일반적으로 정확한 이름으로 지정됩니다).

**프래그먼트 자체는 `extends` 또는 `includes`를 사용할 수 없습니다.** 자체 완결적이어야 합니다.

### Merge semantics

- **`[ssg]` 스칼라** (`runtime`) -- 자식에 있으면 자식이 우선하고, 없으면 상속합니다.
- **`[shared_services.*]`** -- 이름 기준 교체. 부모와 자식이 모두 `postgres`를 정의하면, 자식 항목이 부모 항목을 완전히 교체합니다(필드 단위 병합이 아니라 전체 항목 교체). 자식이 다시 선언하지 않은 부모 서비스는 상속됩니다.
- **`[secrets.*]`** -- 이름 기준 교체이며, 형태는 `[shared_services.*]`와 동일합니다. 같은 이름의 자식 시크릿은 부모의 시크릿 구성을 완전히 재정의합니다.
- **로드 순서** -- `extends` 부모가 먼저 로드되고, 이어서 각 `includes` 프래그먼트가 순서대로 로드된 다음, 마지막에 최상위 파일 자체가 로드됩니다. 충돌 시 뒤의 레이어가 우선합니다.

### `[unset]` -- 상속된 서비스 또는 시크릿 제거

```toml
[ssg]
extends = "Coastfile.ssg-base"

[unset]
shared_services = ["mongodb"]
secrets = ["pg_password"]
```

병합 **이후에** 지정된 항목을 제거하므로, 자식은 부모가 제공하는 무언가를 선택적으로 제거할 수 있습니다. `shared_services`와 `secrets` 키가 모두 지원됩니다.

독립형 SSG Coastfile에 기술적으로 `[unset]`이 포함될 수는 있지만, 조용히 무시됩니다(일반 Coastfile 동작과 동일: unset은 파일이 상속에 참여할 때만 적용됨).

### Cycles

직접 순환(`A`가 `B`를 extends하고 `B`가 `A`를 extends하는 경우, 또는 `A`가 자기 자신을 extends하는 경우)은 `circular extends/includes dependency detected: '<path>'` 오류와 함께 강하게 에러 처리됩니다. 다이아몬드 상속(서로 다른 두 경로가 같은 부모에서 끝나는 경우)은 허용됩니다 -- 방문 집합은 재귀별로 유지되며 반환 시 pop됩니다.

### `[omit]` is not applicable

일반 Coastfile은 compose 파일에서 서비스 / 볼륨을 제거하기 위해 `[omit]`를 지원합니다. SSG에는 제거할 compose 파일이 없습니다 -- `[shared_services.*]` 항목에서 직접 내부 compose를 생성합니다. 대신 상속된 서비스를 제거하려면 `[unset]`을 사용하세요.

### Inline `--config` rejects `extends` / `includes`

`coast ssg build --config '<toml>'`는 상대 경로를 고정할 디스크상의 위치가 없기 때문에 부모 경로를 해석할 수 없습니다. 인라인 TOML에 `extends` / `includes`를 전달하면 `extends and includes require file-based parsing` 오류가 발생합니다. 대신 `-f <file>` 또는 `--working-dir <dir>`를 사용하세요.

### Build artifact is the flattened form

`coast ssg build`는 독립형 TOML을 `~/.coast/ssg/<project>/builds/<id>/ssg-coastfile.toml`에 씁니다. 이 아티팩트에는 상속 이후 병합된 결과가 들어 있으며 `extends`, `includes`, `[unset]` 지시어는 포함되지 않으므로, 부모 / 프래그먼트 파일이 없어도 빌드를 검사하거나 다시 실행할 수 있습니다. `build_id` 해시도 이 평탄화된 형태를 반영하므로, 부모만 변경되어도 캐시가 올바르게 무효화됩니다.

## Example

env에서 추출한 비밀번호를 사용하는 Postgres + Redis:

```toml
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast" }
auto_create_db = true

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["/var/coast-data/redis:/data"]

[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"
services = ["postgres"]
```

## See Also

- [Shared Service Groups](../shared_service_groups/README.md) -- 개념 개요
- [SSG Building](../shared_service_groups/BUILDING.md) -- `coast ssg build`가 이 파일로 수행하는 작업
- [SSG Volumes](../shared_service_groups/VOLUMES.md) -- 볼륨 선언 형태, 권한, 호스트 볼륨 마이그레이션 레시피
- [SSG Secrets](../shared_service_groups/SECRETS.md) -- `[secrets.*]`를 위한 빌드 시 추출 / 실행 시 주입 파이프라인
- [SSG Routing](../shared_service_groups/ROUTING.md) -- canonical / dynamic / virtual 포트
- [Coastfile: Shared Services](SHARED_SERVICES.md) -- consumer 측 `from_group = true` 구문
- [Coastfile: Secrets and Injection](SECRETS.md) -- 일반 Coastfile `[secrets.*]` 참조
- [Coastfile Inheritance](INHERITANCE.md) -- 공통 `extends` / `includes` / `[unset]` 사고 모델
