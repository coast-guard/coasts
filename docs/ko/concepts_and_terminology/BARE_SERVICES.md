# 베어 서비스

프로젝트를 컨테이너화할 수 있다면, 그렇게 해야 합니다. 베어 서비스는 아직 컨테이너화되지 않았고 단기적으로 `Dockerfile` 및 `docker-compose.yml`을 추가하는 것이 현실적이지 않은 프로젝트를 위해 존재합니다. 이는 목적지가 아니라 디딤돌입니다.

컨테이너화된 서비스를 오케스트레이션하는 `docker-compose.yml` 대신, 베어 서비스는 Coastfile에서 셸 명령을 정의할 수 있게 해주며 Coast는 이를 Coast 컨테이너 내부의 경량 슈퍼바이저와 함께 일반 프로세스로 실행합니다.

## 대신 컨테이너화해야 하는 이유

[Docker Compose](RUNTIMES_AND_SERVICES.md) 서비스는 다음을 제공합니다:

- Dockerfile을 통한 재현 가능한 빌드
- 시작 중 Coast가 대기할 수 있는 헬스 체크
- 서비스 간 프로세스 격리
- Docker가 처리하는 볼륨 및 네트워크 관리
- CI, 스테이징, 프로덕션에서 동작하는 이식 가능한 정의

베어 서비스는 이 중 어느 것도 제공하지 않습니다. 프로세스는 동일한 파일시스템을 공유하고, 크래시 복구는 셸 루프이며, "내 컴퓨터에서는 되는데"는 Coast 안에서도 밖에서도 똑같이 발생할 수 있습니다. 프로젝트에 이미 `docker-compose.yml`이 있다면, 그것을 사용하세요.

## 베어 서비스가 의미가 있는 경우

- 한 번도 컨테이너화된 적 없는 프로젝트에 Coast를 도입하고, 워크트리 격리와 포트 관리를 즉시 활용하고 싶을 때
- 프로젝트가 단일 프로세스 도구 또는 CLI이고 Dockerfile이 과할 때
- 컨테이너화를 점진적으로 진행하고 싶을 때 — 베어 서비스로 시작해 나중에 compose로 옮기기

## 구성

베어 서비스는 Coastfile의 `[services.<name>]` 섹션으로 정의합니다. Coastfile은 `compose`와 `[services]`를 **동시에** 정의할 수 없습니다 — 둘은 상호 배타적입니다.

```toml
[coast]
name = "my-app"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
web = 3000
```

각 서비스에는 네 개의 필드가 있습니다:

| Field | Required | Description |
|---|---|---|
| `command` | yes | 실행할 셸 명령 (예: `"npm run dev"`) |
| `port` | no | 서비스가 리스닝하는 포트로, 포트 매핑에 사용 |
| `restart` | no | 재시작 정책: `"no"` (기본값), `"on-failure"`, 또는 `"always"` |
| `install` | no | 시작 전에 실행할 하나 이상의 명령 (예: `"npm install"` 또는 `["npm install", "npm run build"]`) |

### 설정 패키지

베어 서비스는 일반 프로세스로 실행되므로 Coast 컨테이너에는 올바른 런타임이 설치되어 있어야 합니다. `[coast.setup]`을 사용해 시스템 패키지를 선언하세요:

```toml
[coast.setup]
packages = ["nodejs", "npm"]
```

이들은 어떤 서비스가 시작되기 전에 설치됩니다. 이것이 없으면 컨테이너 내부에서 `npm` 또는 `node` 명령이 실패합니다.

### 설치 명령

`install` 필드는 서비스 시작 전에 실행되며, 매 [`coast assign`](ASSIGN.md) (브랜치 전환)마다 다시 실행됩니다. 의존성 설치는 여기에 넣습니다:

```toml
[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
```

설치 명령은 순차적으로 실행됩니다. 어떤 설치 명령이든 실패하면 서비스는 시작되지 않습니다.

### 재시작 정책

- **`no`** — 서비스는 한 번만 실행됩니다. 종료되면 그대로 죽어 있습니다. 원샷 작업이나 수동으로 관리하고 싶은 서비스에 사용하세요.
- **`on-failure`** — 0이 아닌 코드로 종료될 경우 서비스를 재시작합니다. 정상 종료(코드 0)는 그대로 둡니다. 1초에서 30초까지 지수적 백오프를 사용하며, 10회 연속 크래시 이후에는 포기합니다.
- **`always`** — 성공을 포함해 어떤 종료에도 재시작합니다. `on-failure`와 동일한 백오프를 사용합니다. 절대 멈추면 안 되는 장기 실행 서버에 사용하세요.

서비스가 크래시하기 전에 30초 이상 실행되었다면, 재시도 카운터와 백오프는 리셋됩니다 — 한동안은 건강했다고 가정하며 크래시는 새로운 문제로 간주합니다.

## 내부적으로 어떻게 동작하는가

```text
┌─── Coast: dev-1 ──────────────────────────────────────┐
│                                                       │
│   /coast-supervisor/                                  │
│   ├── web.sh          (runs command, tracks PID)      │
│   ├── worker.sh                                       │
│   ├── start-all.sh    (launches all services)         │
│   ├── stop-all.sh     (SIGTERM via PID files)         │
│   └── ps.sh           (checks PID liveness)           │
│                                                       │
│   /var/log/coast-services/                            │
│   ├── web.log                                         │
│   └── worker.log                                      │
│                                                       │
│   No inner Docker daemon images are used.             │
│   Processes run directly on the container OS.         │
└───────────────────────────────────────────────────────┘
```

Coast는 각 서비스에 대한 셸 스크립트 래퍼를 생성하고 DinD 컨테이너 내부의 `/coast-supervisor/`에 배치합니다. 각 래퍼는 자신의 PID를 추적하고, 출력을 로그 파일로 리다이렉트하며, 셸 루프로 재시작 정책을 구현합니다. Docker Compose도 없고, 내부 Docker 이미지도 없으며, 서비스 간 컨테이너 수준의 격리도 없습니다.

`coast ps`는 Docker에 질의하는 대신 PID 생존 여부를 확인하고, `coast logs`는 `docker compose logs`를 호출하는 대신 로그 파일을 tail 합니다. 로그 출력 형식은 compose의 `service | line` 형식과 일치하므로 Coastguard의 UI는 변경 없이 동작합니다.

## 포트

포트 구성은 compose 기반 Coast와 정확히 동일하게 동작합니다. `[ports]`에 서비스가 리스닝하는 포트를 정의하세요:

```toml
[services.web]
command = "npm start"
port = 3000

[ports]
web = 3000
```

[동적 포트](PORTS.md)는 `coast run`에서 할당되며, [`coast checkout`](CHECKOUT.md)은 평소처럼 캐노니컬 포트를 스왑합니다. 유일한 차이점은 서비스 간에 Docker 네트워크가 없다는 점입니다 — 모두 컨테이너의 루프백 또는 `0.0.0.0`에 직접 바인드합니다.

## 브랜치 전환

베어 서비스 Coast에서 `coast assign`을 실행하면 다음이 발생합니다:

1. 실행 중인 모든 서비스가 SIGTERM으로 중지됩니다
2. 워크트리가 새 브랜치로 전환됩니다
3. 설치 명령이 다시 실행됩니다 (예: `npm install`이 새 브랜치의 의존성을 반영)
4. 모든 서비스가 재시작됩니다

이는 compose에서 일어나는 것 — `docker compose down`, 브랜치 전환, 리빌드, `docker compose up` — 과 동일하지만, 컨테이너 대신 셸 프로세스를 사용합니다.

## 제한 사항

- **헬스 체크 없음.** Coast는 헬스 체크를 정의한 compose 서비스처럼 베어 서비스가 "healthy"해질 때까지 기다릴 수 없습니다. 프로세스를 시작하고 잘 되기를 바랄 뿐입니다.
- **서비스 간 격리 없음.** 모든 프로세스는 Coast 컨테이너 내부에서 동일한 파일시스템과 프로세스 네임스페이스를 공유합니다. 오작동하는 서비스가 다른 서비스에 영향을 줄 수 있습니다.
- **빌드 캐싱 없음.** Docker Compose 빌드는 레이어 단위로 캐시됩니다. 베어 서비스 `install` 명령은 assign마다 매번 처음부터 실행됩니다.
- **크래시 복구가 기본적임.** 재시작 정책은 지수적 백오프가 있는 셸 루프를 사용합니다. systemd나 supervisord 같은 프로세스 슈퍼바이저가 아닙니다.
- **서비스에 대한 `[omit]` 또는 `[unset]` 없음.** Coastfile 타입 합성은 compose 서비스에서는 동작하지만, 베어 서비스는 타입된 Coastfile을 통해 개별 서비스를 생략하는 것을 지원하지 않습니다.

## Compose로 마이그레이션

컨테이너화할 준비가 되면, 마이그레이션 경로는 간단합니다:

1. 각 서비스에 대한 `Dockerfile`을 작성합니다
2. 이를 참조하는 `docker-compose.yml`을 만듭니다
3. Coastfile의 `[services.*]` 섹션을 compose 파일을 가리키는 `compose` 필드로 교체합니다
4. 이제 Dockerfile이 처리하는 `[coast.setup]` 패키지를 제거합니다
5. [`coast build`](BUILDS.md)로 리빌드합니다

포트 매핑, [볼륨](VOLUMES.md), [공유 서비스](SHARED_SERVICES.md), [시크릿](SECRETS.md) 구성은 모두 변경 없이 그대로 이어집니다. 바뀌는 것은 서비스가 실행되는 방식뿐입니다.
