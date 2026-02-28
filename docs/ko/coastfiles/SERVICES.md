# 베어 서비스

> **참고:** 베어 서비스는 Coast 컨테이너 내부에서 일반 프로세스로 직접 실행되며 — 컨테이너화되지 않습니다. 서비스가 이미 Docker로 컨테이너화되어 있다면 대신 `compose`를 사용하세요. 베어 서비스는 Dockerfile과 docker-compose.yml을 작성하는 오버헤드를 건너뛰고 싶은 단순한 구성에 가장 적합합니다.

`[services.*]` 섹션은 Coast가 Docker Compose 없이 DinD 컨테이너 내부에서 직접 실행하는 프로세스를 정의합니다. 이는 `compose` 파일을 사용하는 것의 대안이며 — 동일한 Coastfile에서 둘 다 사용할 수는 없습니다.

베어 서비스는 로그 캡처와 선택적 재시작 정책을 포함해 Coast가 감독합니다. 베어 서비스가 어떻게 동작하는지, 그 한계, 그리고 언제 compose로 마이그레이션해야 하는지에 대한 더 깊은 배경은 [Bare Services](../concepts_and_terminology/BARE_SERVICES.md)를 참고하세요.

## 서비스 정의하기

각 서비스는 `[services]` 아래의 이름이 있는 TOML 섹션입니다. `command` 필드는 필수입니다.

```toml
[services.web]
command = "node server.js"
port = 3000
```

### `command` (필수)

실행할 셸 명령입니다. 비어 있거나 공백만으로 구성되어서는 안 됩니다.

```toml
[services.web]
command = "npx next dev --turbopack --port 3000 --hostname 0.0.0.0"
```

### `port`

서비스가 수신하는 포트입니다. 헬스 체크 및 포트 포워딩 통합에 사용됩니다. 지정하는 경우 0이 아니어야 합니다.

```toml
[services.web]
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

### `restart`

프로세스가 종료될 경우의 재시작 정책입니다. 기본값은 `"no"`입니다.

- `"no"` — 재시작하지 않음
- `"on-failure"` — 프로세스가 0이 아닌 코드로 종료될 때만 재시작
- `"always"` — 항상 재시작

```toml
[services.web]
command = "node server.js"
port = 3000
restart = "on-failure"
```

### `install`

서비스를 시작하기 전에 실행할 명령(예: 의존성 설치)입니다. 단일 문자열 또는 문자열 배열을 받을 수 있습니다.

```toml
[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

```toml
[services.web]
install = ["npm install", "npm run build"]
command = "npm start"
port = 3000
```

## compose와의 상호 배타성

Coastfile은 `compose`와 `[services]`를 동시에 정의할 수 없습니다. `[coast]`에 `compose` 필드가 있다면, 어떤 `[services.*]` 섹션을 추가하는 것도 오류입니다. Coastfile마다 한 가지 접근 방식을 선택하세요.

일부 서비스는 compose로 컨테이너화하고 일부는 베어로 실행해야 한다면, 모두에 대해 compose를 사용하세요 — 베어 서비스에서 compose로 이동하는 방법은 [Bare Services의 마이그레이션 가이드](../concepts_and_terminology/BARE_SERVICES.md)를 참고하세요.

## 예시

### 단일 서비스 Next.js 앱

```toml
[coast]
name = "my-frontend"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --turbopack --port 3002 --hostname 0.0.0.0"
port = 3002
restart = "on-failure"

[ports]
web = 3002
```

### 백그라운드 워커가 있는 웹 서버

```toml
[coast]
name = "my-app"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "node server.js"
port = 3000
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
web = 3000
```

### 다단계 설치가 있는 Python 서비스

```toml
[coast]
name = "ml-service"

[coast.setup]
packages = ["python3", "py3-pip"]

[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
restart = "on-failure"

[ports]
api = 8000
```
