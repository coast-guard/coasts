# 체크아웃

체크아웃은 프로젝트의 [canonical ports](PORTS.md)를 어떤 Coast 인스턴스가 소유할지 제어합니다. Coast를 체크아웃하면 `localhost:3000`, `localhost:5432`, 그리고 그 외 모든 canonical port가 해당 인스턴스로 바로 매핑됩니다.

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

체크아웃 전환은 즉시 이루어집니다 — Coast는 가벼운 `socat` 포워더를 종료하고 다시 생성합니다. 어떤 컨테이너도 재시작되지 않습니다.

```bash
coast checkout dev-2   # instant swap

# localhost:3000  ──→  dev-2 web
# localhost:5432  ──→  dev-2 db
```

## Linux 참고

동적 포트는 Linux에서 특별한 권한 없이도 항상 작동합니다.

`1024` 미만의 canonical port는 다릅니다. Coastfile이 `80` 또는 `443` 같은 포트를 선언한 경우, 호스트를 구성하기 전까지 Linux가 `coast checkout`이 해당 포트에 바인딩하는 것을 차단할 수 있습니다. 일반적인 해결 방법은 다음과 같습니다:

- `net.ipv4.ip_unprivileged_port_start` 값을 올리기
- 포워딩 바이너리 또는 프로세스에 bind capability 부여하기

호스트가 바인딩을 거부할 경우 Coast는 이를 명시적으로 보고합니다.

WSL에서는 Coast가 Docker에 의해 게시된 체크아웃 브리지를 사용하므로, Windows 브라우저와 도구가 Sail 같은 Docker Desktop 워크플로와 유사하게 `127.0.0.1`을 통해 체크아웃된 canonical port에 접근할 수 있습니다.

Caddy를 사용하는 로컬 HTTPS 프로젝트의 경우, Coast는 Coast 설치마다 하나의 Caddy 로컬 CA를 재사용합니다. 해당 루트를 한 번 신뢰하면, 같은 설치 아래에서 다시 생성된 워크스페이스도 계속 그것을 사용합니다.

루트 인증서는 다음 위치에 있습니다:

- 일반 설치의 경우 `~/.coast/caddy/pki/authorities/local/root.crt`
- `coast-dev`의 경우 `~/.coast-dev/caddy/pki/authorities/local/root.crt`

이 둘은 의도적으로 분리되어 있으므로, `coast-dev`를 신뢰한다고 해서 일반 `coast` 설치까지 함께 신뢰되는 것은 아니며, 그 반대도 마찬가지입니다.

활성 설치의 루트 인증서를 확인하거나 내보내려면:

```bash
coast cert info
coast cert path
coast cert fingerprint
coast cert export --to ~/Downloads/coast-root.crt
```

Coast는 신뢰 설치를 사용자가 직접 하도록 둡니다. 인증서를 내보낸 다음, 필요에 따라 OS 또는 브라우저의 신뢰 저장소로 가져오세요.

## 체크아웃이 꼭 필요한가요?

반드시 그렇지는 않습니다. 실행 중인 모든 Coast는 항상 자체 동적 포트를 가지며, 무엇이든 체크아웃하지 않아도 언제든지 그 포트를 통해 어떤 Coast에도 접근할 수 있습니다.

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

체크아웃하지 않아도 브라우저에서 `localhost:62217`을 열어 dev-1의 웹 서버에 접근할 수 있습니다. 많은 워크플로에서는 이것만으로도 충분하며, `coast checkout`을 전혀 사용하지 않고도 원하는 만큼 많은 Coast를 실행할 수 있습니다.

## 체크아웃이 유용한 경우

동적 포트만으로는 충분하지 않고 canonical port가 필요한 상황이 있습니다:

- **canonical port에 하드코딩된 클라이언트 애플리케이션.** Coast 외부에서 실행되는 클라이언트 — 예를 들어 호스트의 프런트엔드 개발 서버, 휴대폰의 모바일 앱, 또는 데스크톱 앱 — 가 `localhost:3000` 또는 `localhost:8080`을 기대한다면, 곳곳에서 포트 번호를 바꾸는 것은 비현실적입니다. Coast를 체크아웃하면 어떤 설정도 변경하지 않고 실제 포트를 사용할 수 있습니다.

- **웹훅과 콜백 URL.** Stripe, GitHub, OAuth 제공자 같은 서비스는 등록해 둔 URL로 콜백을 보냅니다 — 보통 `localhost:3000`으로 전달되는 `https://your-ngrok-tunnel.io` 같은 형태입니다. 동적 포트로 전환하면 콜백이 더 이상 도착하지 않습니다. 체크아웃은 테스트 중인 Coast에 대해 canonical port가 활성화되도록 보장합니다.

- **데이터베이스 도구, 디버거, IDE 통합.** 많은 GUI 클라이언트(pgAdmin, DataGrip, TablePlus), 디버거, IDE 실행 구성은 특정 포트로 연결 프로필을 저장합니다. 체크아웃을 사용하면 저장된 프로필은 그대로 두고, 그 뒤에 연결된 Coast만 바꿀 수 있습니다 — 컨텍스트를 전환할 때마다 디버거 attach 대상이나 데이터베이스 연결을 다시 설정할 필요가 없습니다.

## 체크아웃 해제

다른 Coast를 체크아웃하지 않고 canonical port를 해제하려면:

```bash
coast checkout --none
```

이후에는 어떤 Coast도 canonical port를 소유하지 않습니다. 모든 Coast는 계속해서 각자의 동적 포트를 통해 접근할 수 있습니다.

## 한 번에 하나만

한 번에 정확히 하나의 Coast만 체크아웃할 수 있습니다. `dev-1`이 체크아웃된 상태에서 `coast checkout dev-2`를 실행하면, canonical port는 즉시 `dev-2`로 전환됩니다. 중간 공백은 없습니다 — 기존 포워더가 종료되고 같은 작업 안에서 새 포워더가 생성됩니다.

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

동적 포트는 체크아웃의 영향을 받지 않습니다. 바뀌는 것은 canonical port가 가리키는 대상뿐입니다.
