# 동적 포트 환경 변수

모든 Coast 인스턴스는 각 서비스에 할당된 [동적 포트](PORTS.md)를 노출하는 환경 변수 집합을 받습니다. 이러한 변수는 bare 서비스와 compose 컨테이너 내부 모두에서 사용할 수 있으며, 애플리케이션이 런타임에 외부에서 도달 가능한 포트를 발견할 수 있게 해줍니다.

## 이름 지정 규칙

Coast는 `[ports]` 섹션의 논리적 서비스 이름으로부터 변수 이름을 파생합니다:

1. 대문자로 변환
2. 영숫자가 아닌 문자를 밑줄로 치환
3. `_DYNAMIC_PORT` 추가

```text
[ports] key          Environment variable
─────────────        ────────────────────────────
web             →    WEB_DYNAMIC_PORT
postgres        →    POSTGRES_DYNAMIC_PORT
backend-test    →    BACKEND_TEST_DYNAMIC_PORT
svc.v2          →    SVC_V2_DYNAMIC_PORT
```

서비스 이름이 숫자로 시작하면 Coast는 변수 앞에 밑줄을 붙입니다(예: `9svc`는 `_9SVC_DYNAMIC_PORT`가 됨). 빈 이름은 `SERVICE_DYNAMIC_PORT`로 대체됩니다.

## 예시

다음과 같은 Coastfile이 주어졌을 때:

```toml
[ports]
web = 3000
api = 8080
postgres = 5432
```

이 빌드로 생성된 모든 Coast 인스턴스에는 세 개의 추가 환경 변수가 생깁니다:

```text
WEB_DYNAMIC_PORT=62217
API_DYNAMIC_PORT=55681
POSTGRES_DYNAMIC_PORT=56905
```

실제 포트 번호는 `coast run` 시점에 할당되며 인스턴스마다 다릅니다.

## 사용 시점

가장 일반적인 사용 사례는 응답에 자체 URL을 포함하는 서비스를 구성하는 것입니다: 인증 콜백, OAuth 리디렉션 URI, CORS origin, 또는 webhook URL 등이 있습니다. 이러한 서비스는 자신이 수신하는 내부 포트가 아니라 외부 클라이언트가 사용하는 포트를 알아야 합니다.

예를 들어, NextAuth를 사용하는 Next.js 애플리케이션은 외부에서 도달 가능한 주소로 설정된 `AUTH_URL`이 필요합니다. Coast 내부에서 Next.js는 항상 포트 3000에서 수신하지만, 호스트 측 포트는 동적입니다:

```toml
[services.web]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} yarn dev:web"
port = 3000
```

`:-3000` 대체값은 Coast 외부에서 `WEB_DYNAMIC_PORT`가 설정되지 않은 경우에도 명령이 동작함을 의미합니다.

## 우선순위

같은 이름의 환경 변수가 이미 Coast 컨테이너 내에 존재하는 경우(secrets, inject, 또는 compose environment를 통해 설정됨), Coast는 이를 덮어쓰지 않습니다. 기존 값이 우선합니다.

## 가용성

동적 포트 변수는 시작 시 Coast 컨테이너 환경에 주입됩니다. 다음에서 사용할 수 있습니다:

- Bare 서비스 `install` 명령
- Bare 서비스 `command` 프로세스
- Compose 서비스 컨테이너(컨테이너 환경을 통해)
- `coast exec`를 통해 실행되는 명령

이 값들은 인스턴스 수명 동안 변경되지 않습니다. 인스턴스를 중지했다가 다시 시작해도 동일한 동적 포트를 유지합니다.

## 함께 보기

- [Ports](PORTS.md) - canonical 포트와 동적 포트, 그리고 checkout이 이들 사이를 어떻게 전환하는지
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - 인스턴스 간 서브도메인 라우팅과 쿠키 격리
