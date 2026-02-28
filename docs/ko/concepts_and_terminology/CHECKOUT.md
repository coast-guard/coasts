# 체크아웃

체크아웃은 프로젝트의 [캐노니컬 포트](PORTS.md)를 어떤 Coast 인스턴스가 소유하는지 제어합니다. Coast를 체크아웃하면 `localhost:3000`, `localhost:5432`, 그리고 다른 모든 캐노니컬 포트가 곧바로 해당 인스턴스로 매핑됩니다.

```bash
coast checkout dev-1
```

```text
Before checkout:
  localhost:3000  ──→  (nothing)
  localhost:5432  ──→  (nothing)

After checkout:
  localhost:3000  ──→  dev-1 web
  localhost:5432  ──→  dev-1 db
```

체크아웃 전환은 즉시 이루어집니다 — Coast가 가벼운 `socat` 포워더를 종료하고 다시 생성합니다. 어떤 컨테이너도 재시작되지 않습니다.

```bash
coast checkout dev-2   # instant swap

# localhost:3000  ──→  dev-2 web
# localhost:5432  ──→  dev-2 db
```

## 체크아웃이 필요한가요?

반드시 그렇지는 않습니다. 실행 중인 모든 Coast는 항상 자체 동적 포트를 가지며, 아무것도 체크아웃하지 않고도 언제든 그 포트를 통해 어떤 Coast에도 접근할 수 있습니다.

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

체크아웃하지 않고도 브라우저에서 `localhost:62217`을 열어 dev-1의 웹 서버에 접속할 수 있습니다. 이는 많은 워크플로에서 완전히 괜찮으며, `coast checkout`을 전혀 사용하지 않고도 원하는 만큼 많은 Coast를 실행할 수 있습니다.

## 체크아웃이 유용한 경우

동적 포트만으로는 충분하지 않아 캐노니컬 포트가 필요한 상황이 있습니다:

- **캐노니컬 포트로 하드코딩된 클라이언트 애플리케이션.** Coast 밖에서 실행되는 클라이언트(호스트의 프론트엔드 개발 서버, 휴대폰의 모바일 앱, 데스크톱 앱 등)가 `localhost:3000` 또는 `localhost:8080`을 기대한다면, 모든 곳의 포트 번호를 바꾸는 것은 비현실적입니다. Coast를 체크아웃하면 어떤 설정도 바꾸지 않고 실제 포트를 사용할 수 있습니다.

- **웹훅과 콜백 URL.** Stripe, GitHub, OAuth 제공자 같은 서비스는 등록한 URL(대개 `https://your-ngrok-tunnel.io`처럼 `localhost:3000`으로 포워딩되는 주소)로 콜백을 보냅니다. 동적 포트로 바꾸면 콜백이 더 이상 도착하지 않습니다. 체크아웃은 테스트 중인 Coast에 대해 캐노니컬 포트가 활성 상태임을 보장합니다.

- **데이터베이스 도구, 디버거, IDE 통합.** 많은 GUI 클라이언트(pgAdmin, DataGrip, TablePlus), 디버거, IDE 실행 구성은 특정 포트가 포함된 연결 프로필을 저장합니다. 체크아웃을 사용하면 저장된 프로필을 유지한 채로 그 뒤에 있는 Coast만 교체할 수 있습니다 — 컨텍스트를 전환할 때마다 디버거 attach 대상이나 데이터베이스 연결을 다시 구성할 필요가 없습니다.

## 체크아웃 해제

다른 Coast를 체크아웃하지 않고 캐노니컬 포트를 해제하고 싶다면:

```bash
coast checkout --none
```

이후에는 어떤 Coast도 캐노니컬 포트를 소유하지 않습니다. 모든 Coast는 각자의 동적 포트를 통해 계속 접근할 수 있습니다.

## 한 번에 하나만

한 번에 정확히 하나의 Coast만 체크아웃할 수 있습니다. `dev-1`이 체크아웃된 상태에서 `coast checkout dev-2`를 실행하면 캐노니컬 포트가 즉시 `dev-2`로 전환됩니다. 공백은 없습니다 — 기존 포워더를 종료하고 같은 작업에서 새 포워더를 생성합니다.

```text
┌──────────────────────────────────────────────────┐
│  Your machine                                    │
│                                                  │
│  Canonical (checked-out Coast only):             │
│    localhost:3000 ──→ dev-2 web                  │
│    localhost:5432 ──→ dev-2 db                   │
│                                                  │
│  Dynamic (always available):                     │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

동적 포트는 체크아웃의 영향을 받지 않습니다. 바뀌는 것은 캐노니컬 포트가 가리키는 대상뿐입니다.
