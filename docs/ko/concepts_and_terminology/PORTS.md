# 포트

Coast는 Coast 인스턴스의 각 서비스에 대해 두 종류의 포트 매핑을 관리합니다: 표준 포트와 동적 포트입니다.

## 표준 포트

이 포트들은 프로젝트가 일반적으로 실행되는 포트입니다 — `docker-compose.yml` 또는 로컬 개발 설정에 있는 포트들입니다. 예를 들어, 웹 서버의 경우 `3000`, Postgres의 경우 `5432`입니다.

한 번에 오직 하나의 Coast만 표준 포트를 가질 수 있습니다. [체크아웃](CHECKOUT.md)된 Coast가 해당 포트를 받습니다.

```text
coast checkout dev-1

localhost:3000  ──→  dev-1
localhost:5432  ──→  dev-1
```

이는 브라우저, API 클라이언트, 데이터베이스 도구, 테스트 스위트가 모두 평소처럼 정확히 동작한다는 뜻입니다 — 포트 번호를 변경할 필요가 없습니다.

Linux에서는 `1024` 미만의 표준 포트가 [`coast checkout`](CHECKOUT.md)에서 바인딩되기 전에 호스트 설정이 필요할 수 있습니다. 동적 포트에는 이 제한이 없습니다.

## 동적 포트

실행 중인 모든 Coast는 항상 높은 포트 범위(49152–65535) 내에서 자체 동적 포트 세트를 할당받습니다. 이 포트들은 자동으로 할당되며, 어떤 Coast가 체크아웃되어 있는지와 관계없이 항상 접근할 수 있습니다.

```text
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681

coast ports dev-2

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       63104
#   db       5432       57220
```

동적 포트를 사용하면 체크아웃하지 않고도 어떤 Coast든 살펴볼 수 있습니다. 표준 포트에서 dev-1이 체크아웃된 상태에서도 `localhost:63104`를 열어 dev-2의 웹 서버에 접속할 수 있습니다.

## 함께 동작하는 방식

```text
┌──────────────────────────────────────────────────┐
│  Your machine                                    │
│                                                  │
│  Canonical (checked-out Coast only):             │
│    localhost:3000 ──→ dev-1 web                  │
│    localhost:5432 ──→ dev-1 db                   │
│                                                  │
│  Dynamic (always available):                     │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

[체크아웃](CHECKOUT.md) 전환은 즉시 이루어집니다. Coast는 경량 `socat` 포워더를 종료하고 다시 생성합니다. 어떤 컨테이너도 재시작되지 않습니다.

## 동적 포트 환경 변수

Coast는 각 서비스의 동적 포트를 노출하는 환경 변수를 모든 인스턴스에 주입합니다. 변수 이름은 `[ports]` 키에서 파생됩니다: `web`는 `WEB_DYNAMIC_PORT`가 되고, `backend-test`는 `BACKEND_TEST_DYNAMIC_PORT`가 됩니다.

이 변수들은 서비스가 외부에서 접근 가능한 자신의 포트를 알아야 할 때 유용합니다. 예를 들어 인증 콜백 리디렉션을 위한 `AUTH_URL`을 설정하는 경우입니다. 전체 참조는 [동적 포트 환경 변수](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md)를 참고하세요.

## 포트와 원격 Coast

[원격 Coast](REMOTES.md)의 경우, 포트는 추가적인 SSH 터널 계층을 거칩니다. 각 로컬 동적 포트는 `ssh -L`을 통해 대응되는 원격 동적 포트로 포워딩되고, 그 원격 동적 포트는 다시 원격 DinD 컨테이너 내부의 표준 포트에 매핑됩니다. 이는 투명하게 처리되므로 -- `coast ports`와 `coast checkout`은 로컬 인스턴스와 원격 인스턴스 모두에서 동일하게 동작합니다.

## 함께 보기

- [기본 포트 및 DNS](PRIMARY_PORT_AND_DNS.md) - 빠른 링크, 서브도메인 라우팅, URL 템플릿
- [동적 포트 환경 변수](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - 서비스 명령에서 `WEB_DYNAMIC_PORT` 및 관련 변수를 사용하는 방법
- [원격](REMOTES.md) - 원격 Coast에서 포트 포워딩이 동작하는 방식
