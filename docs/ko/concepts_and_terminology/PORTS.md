# 포트

Coast는 Coast 인스턴스의 모든 서비스에 대해 두 가지 종류의 포트 매핑(캐노니컬 포트와 동적 포트)을 관리합니다.

## 캐노니컬 포트

이 포트들은 프로젝트가 평소에 실행되는 포트입니다 — `docker-compose.yml` 또는 로컬 개발 설정에 있는 포트죠. 예를 들어, 웹 서버는 `3000`, Postgres는 `5432`입니다.

한 번에 하나의 Coast만 캐노니컬 포트를 가질 수 있습니다. [체크아웃](CHECKOUT.md)된 Coast가 이를 가져갑니다.

```text
coast checkout dev-1

localhost:3000  ──→  dev-1
localhost:5432  ──→  dev-1
```

즉, 브라우저, API 클라이언트, 데이터베이스 도구, 테스트 스위트가 모두 평소와 완전히 동일하게 동작합니다 — 포트 번호를 바꿀 필요가 없습니다.

## 동적 포트

실행 중인 모든 Coast는 항상 높은 범위(49152–65535)에서 자신만의 동적 포트 세트를 부여받습니다. 이 포트들은 자동으로 할당되며, 어떤 Coast가 체크아웃되어 있든 상관없이 항상 접근할 수 있습니다.

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

동적 포트를 사용하면 체크아웃하지 않고도 어떤 Coast든 들여다볼 수 있습니다. dev-1이 캐노니컬 포트로 체크아웃되어 있는 동안에도 `localhost:63104`를 열어 dev-2의 웹 서버에 접근할 수 있습니다.

## 함께 동작하는 방식

```text
┌──────────────────────────────────────────────────┐
│  내 컴퓨터                                      │
│                                                  │
│  캐노니컬 (체크아웃된 Coast만):                  │
│    localhost:3000 ──→ dev-1 web                  │
│    localhost:5432 ──→ dev-1 db                   │
│                                                  │
│  동적 (항상 사용 가능):                          │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

[체크아웃](CHECKOUT.md) 전환은 즉시 이루어집니다 — Coast가 가벼운 `socat` 포워더를 종료하고 다시 생성합니다. 어떤 컨테이너도 재시작되지 않습니다.

빠른 링크, 서브도메인 라우팅, URL 템플릿에 대해서는 [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md)도 참고하세요.
