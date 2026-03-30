# 포트

Coast는 Coast 인스턴스의 모든 서비스에 대해 두 종류의 포트 매핑을 관리합니다: canonical 포트와 dynamic 포트입니다.

## Canonical 포트

이것들은 프로젝트가 일반적으로 실행되는 포트입니다 — `docker-compose.yml` 또는 로컬 개발 설정에 있는 포트들입니다. 예를 들어, 웹 서버는 `3000`, Postgres는 `5432`입니다.

한 번에 하나의 Coast만 canonical 포트를 가질 수 있습니다. [체크아웃](CHECKOUT.md)된 Coast가 이를 갖습니다.

```text
coast checkout dev-1

localhost:3000  ──→  dev-1
localhost:5432  ──→  dev-1
```

이는 브라우저, API 클라이언트, 데이터베이스 도구, 테스트 스위트가 모두 평소와 정확히 동일하게 작동한다는 뜻입니다 — 포트 번호를 변경할 필요가 없습니다.

Linux에서는 `1024` 미만의 canonical 포트가 [`coast checkout`](CHECKOUT.md)에서 바인딩되기 전에 호스트 설정이 필요할 수 있습니다. Dynamic 포트에는 이러한 제한이 없습니다.

## Dynamic 포트

실행 중인 모든 Coast는 항상 높은 범위(49152–65535) 안의 고유한 dynamic 포트 집합을 받습니다. 이 포트들은 자동으로 할당되며, 어떤 Coast가 체크아웃되어 있는지와 관계없이 항상 접근할 수 있습니다.

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

Dynamic 포트를 사용하면 체크아웃하지 않고도 어떤 Coast든 들여다볼 수 있습니다. dev-1이 canonical 포트에 체크아웃된 상태에서도 `localhost:63104`를 열어 dev-2의 웹 서버에 접속할 수 있습니다.

## 함께 작동하는 방식

```text
┌──────────────────────────────────────────────────┐
│  사용자 머신                                     │
│                                                  │
│  Canonical (체크아웃된 Coast만):                 │
│    localhost:3000 ──→ dev-1 web                  │
│    localhost:5432 ──→ dev-1 db                   │
│                                                  │
│  Dynamic (항상 사용 가능):                       │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

[checkout](CHECKOUT.md) 전환은 즉시 이루어집니다. Coast는 경량 `socat` 포워더를 종료하고 다시 생성합니다. 컨테이너는 재시작되지 않습니다.

## Dynamic 포트 환경 변수

Coast는 각 서비스의 dynamic 포트를 노출하는 환경 변수를 모든 인스턴스에 주입합니다. 변수 이름은 `[ports]` 키에서 파생됩니다: `web`는 `WEB_DYNAMIC_PORT`가 되고, `backend-test`는 `BACKEND_TEST_DYNAMIC_PORT`가 됩니다.

이 변수들은 서비스가 외부에서 도달 가능한 자신의 포트를 알아야 할 때 유용합니다. 예를 들어 인증 콜백 리디렉션을 위한 `AUTH_URL`을 설정할 때 그렇습니다. 전체 참조는 [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md)를 참고하세요.

## See Also

- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - 빠른 링크, 서브도메인 라우팅, URL 템플릿
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - 서비스 명령에서 `WEB_DYNAMIC_PORT` 및 관련 변수를 사용하는 방법
