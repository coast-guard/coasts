# Next.js 애플리케이션

이 레시피는 Postgres와 Redis를 기반으로 하며, 선택적으로 백그라운드 워커나 보조 서비스를 포함하는 Next.js 애플리케이션을 위한 것입니다. 이 스택은 빠른 HMR을 위해 Turbopack과 함께 Next.js를 [bare service](../concepts_and_terminology/BARE_SERVICES.md)로 실행하고, Postgres와 Redis는 호스트에서 [shared services](../concepts_and_terminology/SHARED_SERVICES.md)로 실행되어 모든 Coast 인스턴스가 동일한 데이터를 공유합니다.

이 패턴은 다음과 같은 경우에 잘 맞습니다:

- 프로젝트가 개발 환경에서 Turbopack과 함께 Next.js를 사용함
- 애플리케이션을 지원하는 데이터베이스 및 캐시 계층(Postgres, Redis)이 있음
- 인스턴스별 데이터베이스 설정 없이 여러 Coast 인스턴스를 병렬로 실행하고 싶음
- 응답에 콜백 URL을 포함하는 NextAuth 같은 인증 라이브러리를 사용함

## The Complete Coastfile

```toml
[coast]
name = "my-nextjs-app"
primary_port = "web"
private_paths = ["packages/web/.next"]
worktree_dir = [".worktrees", ".claude/worktrees"]

[coast.setup]
packages = ["nodejs", "npm", "make", "git", "bash"]
run = [
    "npm install -g corepack",
    "corepack enable",
]

# --- Bare services: Next.js and background worker ---

[services.web]
install = [
    "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)",
    "cd /workspace && test -f config.json || echo {} > config.json",
    "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn prisma migrate dev",
]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev"
port = 3000
restart = "on-failure"
cache = ["node_modules"]

[services.worker]
install = "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)"
command = "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev:worker"
restart = "on-failure"
cache = ["node_modules"]

# --- Shared services: Postgres and Redis on the host ---

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["myapp_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_DB = "postgres", POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "postgres" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["myapp_redis_data:/data"]

# --- Secrets: connection strings for bare services ---

[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"

[secrets.redis_url]
extractor = "command"
run = "echo redis://host.docker.internal:6379"
inject = "env:REDIS_URL"

# --- Ports ---

[ports]
web = 3000
postgres = 5432
redis = 6379

# --- Assign: branch-switch behavior ---

[assign]
default = "none"
exclude_paths = ["docs", ".github", "scripts"]

[assign.services]
web = "hot"
worker = "hot"

[assign.rebuild_triggers]
web = ["package.json", "yarn.lock"]
worker = ["package.json", "yarn.lock"]
```

## Project and Setup

```toml
[coast]
name = "my-nextjs-app"
primary_port = "web"
private_paths = ["packages/web/.next"]
worktree_dir = [".worktrees", ".claude/worktrees"]
```

**`private_paths`**는 Next.js에서 매우 중요합니다. Turbopack은 시작 시 `.next/dev/lock`에 lock 파일을 생성합니다. `private_paths`가 없으면 동일한 브랜치의 두 번째 Coast 인스턴스가 이 lock을 보고 시작을 거부합니다. 이를 사용하면 각 인스턴스는 인스턴스별 오버레이 마운트를 통해 자체적으로 격리된 `.next` 디렉터리를 갖게 됩니다. [Private Paths](../concepts_and_terminology/PRIVATE_PATHS.md)를 참조하세요.

**`worktree_dir`**는 git worktree가 존재하는 디렉터리를 나열합니다. 여러 코딩 에이전트(Claude Code, Cursor, Codex)를 사용하는 경우, 각각이 서로 다른 위치에 worktree를 생성할 수 있습니다. 이들을 모두 나열하면 어떤 도구가 만들었는지와 관계없이 Coast가 worktree를 발견하고 할당할 수 있습니다.

```toml
[coast.setup]
packages = ["nodejs", "npm", "make", "git", "bash"]
run = [
    "npm install -g corepack",
    "corepack enable",
]
```

setup 섹션은 bare service에 필요한 시스템 패키지와 도구를 설치합니다. `corepack enable`은 프로젝트의 `packageManager` 필드를 기준으로 yarn 또는 pnpm을 활성화합니다. 이것들은 인스턴스 시작 시가 아니라 Coast 이미지 내부에서 빌드 시 실행됩니다.

## Bare Services

```toml
[services.web]
install = [
    "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)",
    "cd /workspace && test -f config.json || echo {} > config.json",
    "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn prisma migrate dev",
]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev"
port = 3000
restart = "on-failure"
cache = ["node_modules"]
```

**조건부 설치:** `test -f node_modules/.yarn-state.yml || make yarn` 패턴은 `node_modules`가 이미 존재하면 의존성 설치를 건너뜁니다. 이를 통해 의존성이 변경되지 않았을 때 브랜치 전환이 빨라집니다. [Bare Service Optimization](../concepts_and_terminology/BARE_SERVICE_OPTIMIZATION.md)을 참조하세요.

**`cache`:** worktree 전환 간에 `node_modules`를 유지하여 `yarn install`이 처음부터 다시 실행되는 대신 점진적으로 실행되도록 합니다.

**동적 포트와 함께 사용하는 `AUTH_URL`:** NextAuth(또는 유사한 인증 라이브러리)를 사용하는 Next.js 애플리케이션은 응답에 콜백 URL을 포함합니다. Coast 내부에서 Next.js는 포트 3000에서 수신하지만, 호스트 측 포트는 동적입니다. Coast는 `[ports]`의 `web` 키에서 파생된 `WEB_DYNAMIC_PORT`를 컨테이너 환경에 자동으로 주입합니다. `:-3000` 폴백은 동일한 명령이 Coast 외부에서도 동작하도록 합니다. [Dynamic Port Environment Variables](../concepts_and_terminology/DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md)를 참조하세요.

**`host.docker.internal`:** bare service는 `localhost`를 통해 shared service에 접근할 수 없습니다. shared service는 호스트 Docker 데몬에서 실행되기 때문입니다. `host.docker.internal`은 Coast 컨테이너 내부에서 호스트를 가리킵니다.

## Shared Services

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["myapp_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_DB = "postgres", POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "postgres" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["myapp_redis_data:/data"]
```

Postgres와 Redis는 호스트 Docker 데몬에서 [shared services](../concepts_and_terminology/SHARED_SERVICES.md)로 실행됩니다. 모든 Coast 인스턴스가 동일한 데이터베이스에 연결하므로 사용자, 세션, 데이터가 인스턴스 간에 공유됩니다. 이를 통해 각 인스턴스마다 별도로 회원가입해야 하는 문제를 피할 수 있습니다.

프로젝트에 이미 Postgres와 Redis가 포함된 `docker-compose.yml`이 있다면, 대신 `compose`를 사용하고 볼륨 전략을 `shared`로 설정할 수 있습니다. bare-service Coastfile에서는 관리할 compose 파일이 없기 때문에 shared service가 더 단순합니다.

## Secrets

```toml
[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"

[secrets.redis_url]
extractor = "command"
run = "echo redis://host.docker.internal:6379"
inject = "env:REDIS_URL"
```

이 설정은 빌드 시 `DATABASE_URL`과 `REDIS_URL`을 Coast 컨테이너 환경에 주입합니다. 연결 문자열은 `host.docker.internal`을 통해 shared service를 가리킵니다.

`command` extractor는 셸 명령을 실행하고 stdout을 캡처합니다. 여기서는 단순히 정적 문자열을 echo 하지만, vault에서 읽거나 CLI 도구를 실행하거나 값을 동적으로 계산하는 데 사용할 수도 있습니다.

bare service의 `command` 필드도 이러한 변수를 인라인으로 설정한다는 점에 유의하세요. 인라인 값이 우선하지만, 주입된 secret은 `install` 단계와 `coast exec` 세션의 기본값으로 사용됩니다.

## Assign Strategies

```toml
[assign]
default = "none"
exclude_paths = ["docs", ".github", "scripts"]

[assign.services]
web = "hot"
worker = "hot"

[assign.rebuild_triggers]
web = ["package.json", "yarn.lock"]
worker = ["package.json", "yarn.lock"]
```

**`default = "none"`**는 브랜치 전환 시 shared service와 인프라를 그대로 둡니다. 코드에 의존하는 서비스에만 assign 전략이 적용됩니다.

**Next.js와 워커에 대한 `hot`:** Turbopack이 포함된 Next.js는 내장 hot module replacement를 제공합니다. Coast가 `/workspace`를 새 worktree로 다시 마운트하면 Turbopack이 파일 변경을 감지하고 자동으로 다시 컴파일합니다. 프로세스 재시작이 필요하지 않습니다. `tsc --watch` 또는 `nodemon`을 사용하는 백그라운드 워커도 파일 watcher를 통해 변경 사항을 감지합니다.

**`rebuild_triggers`:** 브랜치 간에 `package.json` 또는 `yarn.lock`이 변경되면 서비스가 다시 시작되기 전에 해당 서비스의 `install` 명령이 다시 실행됩니다. 이를 통해 패키지가 추가되거나 제거된 브랜치 전환 후에도 의존성이 최신 상태로 유지됩니다.

**`exclude_paths`:** 서비스에 필요하지 않은 디렉터리를 건너뛰어 최초 worktree 부트스트랩 속도를 높입니다. 문서, CI 설정, 스크립트는 제외해도 안전합니다.

## Adapting This Recipe

**백그라운드 워커 없음:** `[services.worker]` 섹션과 해당 assign 항목을 제거하세요. 나머지 Coastfile은 수정 없이 그대로 동작합니다.

**여러 Next.js 앱이 있는 모노레포:** 각 앱의 `.next` 디렉터리에 대해 `private_paths` 항목을 추가하세요. 각 bare service는 적절한 `command`와 `port`를 가진 자체 `[services.*]` 섹션을 갖습니다.

**yarn 대신 pnpm 사용:** `make yarn`을 pnpm 설치 명령으로 바꾸세요. pnpm이 의존성을 다른 위치(예: `.pnpm-store`)에 저장한다면 `cache` 필드도 그에 맞게 조정하세요.

**shared service 없음:** 인스턴스별 데이터베이스를 선호한다면 `[shared_services]`와 `[secrets]` 섹션을 제거하세요. `docker-compose.yml`에 Postgres와 Redis를 추가하고, `[coast]` 섹션에 `compose`를 설정한 다음, 격리를 제어하기 위해 [volume strategies](../coastfiles/VOLUMES.md)를 사용하세요. 인스턴스별 데이터에는 `strategy = "isolated"`를, 공유 데이터에는 `strategy = "shared"`를 사용하세요.

**추가 인증 제공자:** 인증 라이브러리가 콜백 URL에 `AUTH_URL` 외의 환경 변수를 사용한다면, 서비스 명령에서 해당 변수에도 동일한 `${WEB_DYNAMIC_PORT:-3000}` 패턴을 적용하세요.
