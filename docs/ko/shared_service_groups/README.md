# 공유 서비스 그룹

공유 서비스 그룹(SSG)은 Docker-in-Docker 컨테이너로, 프로젝트의 인프라 서비스인 Postgres, Redis, MongoDB, 즉 원래 `[shared_services]` 아래에 두었을 모든 것을, 이를 소비하는 [Coast](../concepts_and_terminology/COASTS.md) 인스턴스들과 분리된 한 곳에서 실행합니다. 모든 Coast 프로젝트는 자체 SSG를 가지며, 이름은 `<project>-ssg`이고, 프로젝트의 `Coastfile`와 나란히 있는 `Coastfile.shared_service_groups`로 선언됩니다.

각 소비자 인스턴스(`dev-1`, `dev-2`, ...)는 안정적인 가상 포트를 통해 자기 프로젝트의 SSG에 연결되므로, SSG를 다시 빌드해도 소비자에 변경 소음이 발생하지 않습니다. 각 Coast 내부에서는 계약이 바뀌지 않습니다: `postgres:5432`는 여러분의 공유 Postgres로 해석되며, 애플리케이션 코드는 특별한 일이 일어나고 있다는 사실을 전혀 알지 못합니다.

## 왜 SSG가 필요한가

원래의 [공유 서비스](../concepts_and_terminology/SHARED_SERVICES.md) 패턴은 호스트 Docker 데몬에서 하나의 인프라 컨테이너를 시작하고, 이를 프로젝트의 모든 소비자 인스턴스에서 공유합니다. 하나의 프로젝트에는 이 방식이 잘 작동합니다. 문제는 **서로 다른 두 프로젝트**가 각각 `5432`에 Postgres를 선언할 때 시작됩니다: 두 프로젝트 모두 같은 호스트 포트를 바인딩하려고 시도하고, 두 번째 프로젝트는 실패합니다.

```text
SSG가 없을 때(프로젝트 간 호스트 포트 충돌):

호스트 Docker 데몬
+-- cg-coasts-postgres            (프로젝트 "cg"가 호스트 :5432에 바인딩)
+-- filemap-coasts-postgres       (프로젝트 "filemap"이 :5432 시도 -- 실패)
+-- cg-coasts-dev-1               --> cg-coasts-postgres
+-- cg-coasts-dev-2               --> cg-coasts-postgres   (같은 프로젝트 인스턴스끼리는 문제 없이 공유)
```

SSG는 각 프로젝트의 인프라를 자체 DinD로 끌어올려 이 문제를 해결합니다. Postgres는 여전히 표준 `:5432`에서 수신 대기하지만, 호스트가 아니라 SSG 내부에서 그렇게 합니다. SSG 컨테이너는 임의의 동적 호스트 포트로 게시되고, 데몬이 관리하는 가상 포트 socat(`42000-43000` 대역)이 소비자 트래픽을 그쪽으로 브리지합니다. 두 프로젝트는 각각 표준 5432에 Postgres를 둘 수 있습니다. 둘 중 어느 것도 호스트 5432에 바인딩하지 않기 때문입니다:

```text
SSG가 있을 때(프로젝트별, 프로젝트 간 충돌 없음):

호스트 Docker 데몬
+-- cg-ssg                        (프로젝트 "cg" -- DinD)
|     +-- postgres                (내부 :5432, 호스트 동적 54201, 가상 포트 42000)
|     +-- redis                   (내부 :6379, 호스트 동적 54202, 가상 포트 42001)
+-- filemap-ssg                   (프로젝트 "filemap" -- DinD, 충돌 없음)
|     +-- postgres                (내부 :5432, 호스트 동적 54250, 가상 포트 42002)
|     +-- redis                   (내부 :6379, 호스트 동적 54251, 가상 포트 42003)
+-- cg-coasts-dev-1               --> hg-internal:42000 --> cg-ssg postgres
+-- cg-coasts-dev-2               --> hg-internal:42000 --> cg-ssg postgres
+-- filemap-coasts-dev-1          --> hg-internal:42002 --> filemap-ssg postgres
```

각 프로젝트의 SSG는 자기 자신의 데이터, 자기 자신의 이미지 버전, 자기 자신의 시크릿을 소유합니다. 두 프로젝트는 상태를 공유하지 않고, 포트를 두고 경쟁하지 않으며, 서로의 데이터를 보지도 못합니다. 각 소비자 Coast 내부에서는 계약이 바뀌지 않습니다: 앱 코드는 `postgres:5432`에 연결하면 자기 프로젝트의 Postgres에 도달합니다 -- 나머지는 라우팅 계층(참고: [라우팅](ROUTING.md))이 처리합니다.

## 빠른 시작

`Coastfile.shared_service_groups`는 프로젝트의 `Coastfile`와 나란히 존재합니다. 프로젝트 이름은 일반 Coastfile의 `[coast].name`에서 가져오므로 -- 다시 반복해서 적지 않습니다.

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

# 선택 사항: 환경 변수, 키체인 또는 1Password에서 시크릿을 추출하여
# 빌드 시점에 가져오고 실행 시점에 SSG에 주입할 수 있습니다. SECRETS.md를 참고하세요.
[secrets.pg_password]
extractor = "env"
inject = "env:POSTGRES_PASSWORD"
var = "MY_PG_PASSWORD"
```

빌드하고 실행합니다:

```bash
coast ssg build       # 파싱, 이미지 가져오기, 시크릿 추출, 아티팩트 쓰기
coast ssg run         # <project>-ssg 시작, 시크릿 구체화, compose up
coast ssg ps          # 서비스 상태 표시
```

소비자 Coast가 이를 가리키도록 설정합니다:

```toml
# 같은 프로젝트의 Coastfile
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true
inject = "env:DATABASE_URL"

[shared_services.redis]
from_group = true
```

그다음 `coast build && coast run dev-1`를 실행합니다. SSG가 아직 실행 중이 아니라면 자동으로 시작됩니다. `dev-1`의 앱 컨테이너 내부에서 `postgres:5432`는 SSG의 Postgres로 해석되고 `$DATABASE_URL`은 표준 연결 문자열로 설정됩니다.

## 참조

| 페이지 | 다루는 내용 |
|---|---|
| [Building](BUILDING.md) | `coast ssg build`의 전체 과정, 프로젝트별 아티팩트 레이아웃, 시크릿 추출, `Coastfile.shared_service_groups` 탐색 규칙, 그리고 특정 빌드에 프로젝트를 고정하는 방법 |
| [Lifecycle](LIFECYCLE.md) | `run` / `start` / `stop` / `restart` / `rm` / `ps` / `logs` / `exec`, 프로젝트별 `<project>-ssg` 컨테이너, `coast run` 시 자동 시작, 그리고 프로젝트 간 목록 확인을 위한 `coast ssg ls` |
| [Routing](ROUTING.md) | 표준 / 동적 / 가상 포트, 호스트 socat 계층, 앱에서 내부 서비스까지의 전체 hop-by-hop 체인, 그리고 원격 소비자 대칭 터널 |
| [Volumes](VOLUMES.md) | 호스트 바인드 마운트, 대칭 경로, 내부 네임드 볼륨, 권한, `coast ssg doctor` 명령, 그리고 기존 호스트 볼륨을 SSG로 마이그레이션하는 방법 |
| [Consuming](CONSUMING.md) | `from_group = true`, 허용 및 금지 필드, 충돌 감지, `auto_create_db`, `inject`, 그리고 원격 소비자 |
| [Secrets](SECRETS.md) | SSG Coastfile의 `[secrets.<name>]`, 빌드 시점 추출기 파이프라인, `compose.override.yml`을 통한 실행 시점 주입, 그리고 `coast ssg secrets clear` 동사 |
| [Checkout](CHECKOUT.md) | `coast ssg checkout` / `uncheckout`으로 SSG의 표준 포트를 호스트에 바인딩하여 호스트의 모든 것(psql, redis-cli, IDE)이 이에 도달할 수 있게 하는 방법 |
| [CLI](CLI.md) | 모든 `coast ssg` 하위 명령의 한 줄 요약 |

## 함께 보기

- [공유 서비스](../concepts_and_terminology/SHARED_SERVICES.md) -- SSG가 일반화한 인스턴스 내부 인라인 패턴
- [공유 서비스 Coastfile 참조](../coastfiles/SHARED_SERVICES.md) -- `from_group`을 포함한 소비자 측 TOML 문법
- [Coastfile: 공유 서비스 그룹](../coastfiles/SHARED_SERVICE_GROUPS.md) -- `Coastfile.shared_service_groups`의 전체 스키마
- [포트](../concepts_and_terminology/PORTS.md) -- 표준 포트와 동적 포트
