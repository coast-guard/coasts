# Coastfiles

Coastfile은 프로젝트 루트에 위치하는 TOML 구성 파일입니다. 이 파일은 해당 프로젝트를 위한 격리된 개발 환경을 빌드하고 실행하는 데 Coast가 알아야 하는 모든 정보를 알려줍니다 — 어떤 서비스를 실행할지, 어떤 포트를 포워딩할지, 데이터를 어떻게 처리할지, 시크릿을 어떻게 관리할지 등을 포함합니다.

모든 Coast 프로젝트에는 최소 하나의 Coastfile이 필요합니다. 파일 이름은 항상 `Coastfile`입니다(대문자 C, 확장자 없음). 서로 다른 워크플로를 위한 변형이 필요하다면, `Coastfile.light` 또는 `Coastfile.snap`처럼 타입이 지정된 Coastfile을 만들고 [기본 파일을 상속](INHERITANCE.md)할 수 있습니다.

Coastfile이 Coast의 나머지 부분과 어떤 관계인지 더 깊이 이해하려면 [Coasts](../concepts_and_terminology/COASTS.md) 및 [Builds](../concepts_and_terminology/BUILDS.md)를 참고하세요.

## Quickstart

가장 작은 형태의 Coastfile:

```toml
[coast]
name = "my-app"
```

이렇게 하면 `coast exec`으로 들어갈 수 있는 DinD 컨테이너가 제공됩니다. 대부분의 프로젝트는 `compose` 참조 또는 [bare services](SERVICES.md) 중 하나를 원할 것입니다:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
web = 3000
api = 8080
```

또는 compose 없이 bare services를 사용하는 경우:

```toml
[coast]
name = "my-app"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
restart = "on-failure"

[ports]
web = 3000
```

`coast build`를 실행한 다음 `coast run dev-1`을 실행하면 격리된 환경이 준비됩니다.

## Example Coastfiles

### Simple bare-service project

compose 파일이 없는 Next.js 앱입니다. Coast가 Node를 설치하고, `npm install`을 실행한 다음, 개발 서버를 직접 시작합니다.

```toml
[coast]
name = "my-crm"
runtime = "dind"

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

### Full-stack compose project

공유 데이터베이스, 시크릿, 볼륨 전략, 사용자 정의 설정을 갖춘 멀티 서비스 프로젝트입니다.

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
primary_port = "web"

[coast.setup]
packages = ["nodejs", "npm", "python3", "curl", "git", "bash", "ca-certificates", "wget"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]

[ports]
web = 3000
backend = 8080
postgres = 5432
redis = 6379

[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass" }

[shared_services.redis]
image = "redis:7"
ports = [6379]

[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"

[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"

[omit]
services = ["monitoring", "admin-panel", "nginx-proxy"]

[assign]
default = "none"
[assign.services]
backend = "hot"
web = "hot"
```

### Lightweight test variant (inheritance)

기본 Coastfile을 확장하지만 백엔드 테스트 실행에 필요한 것만 남기도록 축소한 버전입니다. 포트도 없고, 공유 서비스도 없으며, 데이터베이스는 격리됩니다.

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "backend", "postgres", "redis"]
shared_services = ["postgres", "redis"]

[omit]
services = ["redis", "backend", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[assign]
default = "none"
[assign.services]
backend-test = "rebuild"
```

### Snapshot-seeded variant

각 coast 인스턴스는 호스트의 기존 데이터베이스 볼륨 복사본으로 시작한 뒤, 이후에는 독립적으로 분기됩니다.

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis", "mongodb"]

[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "infra_redis_data"
service = "redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
snapshot_source = "infra_mongodb_data"
service = "mongodb"
mount = "/data/db"
```

## Conventions

- 파일 이름은 반드시 `Coastfile`이어야 하며(대문자 C, 확장자 없음), 프로젝트 루트에 위치해야 합니다.
- 타입이 지정된 변형은 `Coastfile.{type}` 패턴을 사용합니다 — 예: `Coastfile.light`, `Coastfile.snap`. [Inheritance and Types](INHERITANCE.md)를 참고하세요.
- 예약된 이름 `Coastfile.default`는 허용되지 않습니다.
- 전체적으로 TOML 문법을 사용합니다. 모든 섹션 헤더는 `[brackets]`를 사용하고, 이름 있는 항목은 `[section.name]`을 사용합니다(array-of-tables 아님).
- 같은 Coastfile에서 `compose`와 `[services]`를 동시에 사용할 수 없습니다 — 하나를 선택하세요.
- 상대 경로(`compose`, `root` 등)는 Coastfile의 상위 디렉터리를 기준으로 해석됩니다.

## Reference

| Page | Sections | What it covers |
|------|----------|----------------|
| [Project and Setup](PROJECT.md) | `[coast]`, `[coast.setup]` | 이름, compose 경로, 런타임, worktree 디렉터리, 컨테이너 설정 |
| [Worktree Directories](WORKTREE_DIR.md) | `worktree_dir`, `default_worktree_dir` | 로컬 및 외부 worktree 디렉터리, 틸드 경로, Codex/Claude 통합 |
| [Ports](PORTS.md) | `[ports]`, `[egress]` | 포트 포워딩, egress 선언, 기본 포트 |
| [Volumes](VOLUMES.md) | `[volumes.*]` | 격리, 공유, 스냅샷 시드 볼륨 전략 |
| [Shared Services](SHARED_SERVICES.md) | `[shared_services.*]` | 호스트 수준 데이터베이스 및 인프라 서비스 |
| [Secrets](SECRETS.md) | `[secrets.*]`, `[inject]` | 시크릿 추출, 주입, 호스트 env/file 포워딩 |
| [Bare Services](SERVICES.md) | `[services.*]` | Docker Compose 없이 프로세스를 직접 실행 |
| [Agent Shell](AGENT_SHELL.md) | `[agent_shell]` | 컨테이너화된 에이전트 TUI 런타임 |
| [MCP Servers](MCP.md) | `[mcp.*]`, `[mcp_clients.*]` | 내부 및 호스트 프록시 MCP 서버, 클라이언트 커넥터 |
| [Assign](ASSIGN.md) | `[assign]` | 서비스별 브랜치 전환 동작 |
| [Inheritance and Types](INHERITANCE.md) | `extends`, `includes`, `[unset]`, `[omit]` | 타입이 지정된 Coastfile, 조합, 오버라이드 |
