# 기본 포트 & DNS

기본 포트(primary port)는 서비스 중 하나(일반적으로 웹 프런트엔드)에 대한 빠른 링크를 만들어주는 선택적 편의 기능입니다. Coastguard에서는 클릭 가능한 배지로, `coast ports`에서는 별표가 표시된 항목으로 나타납니다. 포트가 동작하는 방식을 바꾸지는 않으며, 강조 표시할 하나를 선택해줄 뿐입니다.

## 기본 포트 설정하기

Coastfile의 `[coast]` 섹션에 `primary_port`를 추가하고, [`[ports]`](PORTS.md)의 키를 참조하세요:

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
backend = 8080
```

프로젝트에 포트가 하나뿐이라면 Coast가 이를 기본 포트로 자동 감지하므로, 명시적으로 설정할 필요가 없습니다.

Coastguard의 Ports 탭에서 어떤 서비스 옆의 별 아이콘을 클릭해 기본 포트를 토글할 수도 있고, CLI에서 `coast ports set-primary`로 설정할 수도 있습니다. 이 설정은 빌드 단위이므로, 동일한 빌드에서 생성된 모든 인스턴스는 같은 기본 포트를 공유합니다.

## 무엇이 가능해지나요

```text
coast ports dev-1

  SERVICE    CANONICAL  DYNAMIC
  ★ web      3000       62217
    backend  8080       63889
```

별표가 표시된 서비스가 기본 포트입니다. Coastguard에서는 인스턴스 이름 옆에 클릭 가능한 배지로 표시되며, 한 번 클릭하면 브라우저에서 앱이 열립니다.

이는 특히 다음에 유용합니다:

- **호스트 측 에이전트** — AI 에이전트에게 변경 사항을 확인할 단일 URL을 제공하세요. "localhost:62217을 열어"라고 말하는 대신, 기본 포트 URL은 `coast ls`와 데몬 API에서 프로그래밍 방식으로 사용할 수 있습니다.
- **브라우저 MCP** — 에이전트가 UI 변경을 검증하기 위해 브라우저 MCP를 사용한다면, 기본 포트 URL이 가리켜야 할 표준 대상입니다.
- **빠른 반복** — 가장 자주 확인하는 서비스에 원클릭으로 접근.

기본 포트는 완전히 선택 사항입니다. 없어도 모든 것이 동작하며, 더 빠른 탐색을 위한 삶의 질(QoL) 기능입니다.

## 서브도메인 라우팅

격리된 데이터베이스를 가진 여러 Coast 인스턴스를 실행하면, 브라우저에서 모두 `localhost`를 공유합니다. 이는 `localhost:62217`(dev-1)이 설정한 쿠키가 `localhost:63104`(dev-2)에서도 보인다는 뜻입니다. 앱이 세션 쿠키를 사용한다면, 한 인스턴스에 로그인하는 것이 다른 인스턴스에 영향을 줄 수 있습니다.

서브도메인 라우팅은 각 인스턴스에 고유한 origin을 부여함으로써 이를 해결합니다:

```text
Without subdomain routing:
  dev-1 web  →  http://localhost:62217
  dev-2 web  →  http://localhost:63104
  (cookies shared — both are "localhost")

With subdomain routing:
  dev-1 web  →  http://dev-1.localhost:62217
  dev-2 web  →  http://dev-2.localhost:63104
  (cookies isolated — different subdomains)
```

프로젝트별로 Coastguard Ports 탭(페이지 하단의 토글)에서 활성화하거나, 데몬 설정 API를 통해 활성화할 수 있습니다.

### 트레이드오프: CORS

단점은 애플리케이션에서 CORS 조정이 필요할 수 있다는 점입니다. `dev-1.localhost:3000`의 프런트엔드가 `dev-1.localhost:8080`로 API 요청을 보내면, 브라우저는 포트가 다르기 때문에 이를 교차 출처(cross-origin)로 취급합니다. 대부분의 개발 서버는 이미 이를 처리하지만, 서브도메인 라우팅을 활성화한 뒤 CORS 오류가 보인다면 애플리케이션의 허용 origin 설정을 확인하세요.

## URL 템플릿

각 서비스에는 링크가 생성되는 방식을 제어하는 URL 템플릿이 있습니다. 기본값은 다음과 같습니다:

```text
http://localhost:<port>
```

`<port>` 플레이스홀더는 실제 포트 번호로 치환됩니다 — 인스턴스가 [체크아웃](CHECKOUT.md)되어 있을 때는 canonical 포트, 그렇지 않으면 dynamic 포트입니다. 서브도메인 라우팅이 활성화되면 `localhost:`는 `{instance}.localhost:`로 대체됩니다.

Coastguard Ports 탭에서 서비스별로 템플릿을 커스터마이즈할 수 있습니다(각 서비스 옆의 연필 아이콘). 개발 서버가 HTTPS, 커스텀 호스트명, 또는 비표준 URL 스킴을 사용하는 경우 유용합니다:

```text
https://my-service.localhost:<port>
```

템플릿은 데몬 설정에 저장되며 재시작 후에도 유지됩니다.

## DNS 설정

대부분의 브라우저는 기본적으로 `*.localhost`를 `127.0.0.1`로 해석하므로, 별도의 DNS 설정 없이도 서브도메인 라우팅이 동작합니다.

커스텀 도메인 해석이 필요하다면(예: `*.localcoast`), Coast에는 내장 DNS 서버가 포함되어 있습니다. 한 번만 설정하세요:

```bash
coast dns setup    # writes /etc/resolver/localcoast (requires sudo)
coast dns status   # check if DNS is configured
coast dns remove   # remove the resolver entry
```

이는 선택 사항이며, 브라우저에서 `*.localhost`가 동작하지 않거나 커스텀 TLD를 원할 때만 필요합니다.
