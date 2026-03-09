# 풀스택 모노레포

이 레시피는 공유 데이터베이스와 캐시 계층을 기반으로 여러 웹 애플리케이션을 포함하는 대규모 모노레포를 위한 것입니다. 이 스택은 무거운 백엔드 서비스(Rails, Sidekiq, SSR)에는 Docker Compose를 사용하고, Vite 개발 서버는 DinD 호스트에서 베어 서비스로 실행합니다. Postgres와 Redis는 호스트 Docker 데몬에서 공유 서비스로 실행되므로, 모든 Coast 인스턴스가 인프라를 중복으로 띄우지 않고 동일한 인프라에 연결합니다.

이 패턴은 다음과 같은 경우에 잘 동작합니다:

- 모노레포에 데이터베이스를 공유하는 여러 앱이 포함되어 있는 경우
- 각 Coast 인스턴스가 각자 Postgres와 Redis를 실행하지 않는 가벼운 Coast 인스턴스를 원할 때
- 프론트엔드 개발 서버가 `host.docker.internal`을 통해 compose 컨테이너 내부에서 접근 가능해야 할 때
- 호스트 측 MCP 통합이 `localhost:5432`에 연결되어 있고, 이를 변경 없이 계속 동작시키고 싶을 때

## 전체 Coastfile

아래는 전체 Coastfile입니다. 각 섹션은 아래에서 자세히 설명합니다.

```toml
[coast]
name = "acme"
compose = "./infra/docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "bash"]
run = ["npm install -g yarn"]

[ports]
api = 3000
web = 3002
vite-web = 3040
vite-api = 3036

[healthcheck]
web = "/"
api = "/"

# --- Bare services: Vite dev servers on the DinD host ---

[services.vite-web]
install = "cd /workspace && yarn install --immutable 2>/dev/null || yarn install"
command = "cd /workspace && yarn workspace @acme/web run dev"
port = 3040
restart = "on-failure"
cache = ["node_modules"]

[services.vite-api]
command = "cd /workspace && yarn workspace @acme/api run dev"
port = 3036
restart = "on-failure"

# --- Shared services: Postgres and Redis on the host daemon ---

[shared_services.db]
image = "postgres:15.3-alpine"
ports = [5432]
volumes = ["infra_postgres:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "password" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis:/data"]

# --- Volumes: shared caches across all instances ---

[volumes.bundle]
strategy = "shared"
service = "api-rails"
mount = "/usr/local/bundle"

[volumes.api_rails_cache]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/tmp/cache"

[volumes.api_assets]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/public/assets"

[volumes.web_rails_cache]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/tmp/cache"

[volumes.web_assets]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/public/assets"

# --- Secrets and injection ---

[secrets.compose_profiles]
extractor = "command"
run = "echo api,web"
inject = "env:COMPOSE_PROFILES"

[secrets.uid]
extractor = "command"
run = "id -u"
inject = "env:UID"

[secrets.gid]
extractor = "command"
run = "id -g"
inject = "env:GID"

[inject]
env = ["USER", "BUNDLE_GEMS__CONTRIBSYS__COM"]

# --- Assign: branch-switch behavior ---

[assign]
default = "none"
exclude_paths = [
    ".coasts",
    ".yarn",
    ".github",
    "docs",
    "scripts",
    "cli",
    "deploy",
    "tools",
    "mobile",
    "extensions",
]

[assign.services]
web-rails = "hot"
web-ssr = "hot"
web-sidekiq = "restart"
api-rails = "hot"
api-sidekiq = "restart"

[assign.rebuild_triggers]
web-rails = ["infra/Dockerfile", "web/Gemfile", "web/Gemfile.lock", "web/package.json"]
api-rails = ["infra/Dockerfile", "api/Gemfile", "api/Gemfile.lock", "api/package.json"]
```

## Project and Compose

```toml
[coast]
name = "acme"
compose = "./infra/docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "bash"]
run = ["npm install -g yarn"]
```

`compose` 필드는 기존 Docker Compose 파일을 가리킵니다. Coast는 `coast run` 시 DinD 컨테이너 내부에서 `docker compose up -d`를 실행하므로, 백엔드 서비스(Rails 서버, Sidekiq 워커, SSR 프로세스)가 자동으로 시작됩니다.

`[coast.setup]`은 DinD 호스트 자체에 패키지를 설치합니다 — compose 컨테이너 내부가 아닙니다. 이는 호스트에서 직접 실행되는 베어 서비스(Vite 개발 서버)에 필요합니다. compose 서비스들은 평소처럼 Dockerfile로부터 런타임을 받습니다.

## 공유 서비스

```toml
[shared_services.db]
image = "postgres:15.3-alpine"
ports = [5432]
volumes = ["infra_postgres:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "password" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis:/data"]
```

Postgres와 Redis는 각 Coast 내부에서 실행하는 대신 [공유 서비스](../concepts_and_terminology/SHARED_SERVICES.md)로 선언됩니다. 이는 이들이 호스트 Docker 데몬에서 실행되고, 모든 Coast 인스턴스가 브리지 네트워크를 통해 연결한다는 뜻입니다.

**왜 compose 내부 데이터베이스 대신 공유 서비스를 쓰나요?**

- **더 가벼운 인스턴스.** 각 Coast가 자체 Postgres와 Redis 컨테이너를 띄우는 것을 생략하여 메모리와 시작 시간을 절약합니다.
- **호스트 볼륨 재사용.** `volumes` 필드는 기존 Docker 볼륨(로컬에서 `docker-compose up`으로 생성된 것)을 참조합니다. 이미 가지고 있는 모든 데이터가 즉시 사용 가능하며, 시딩이나 마이그레이션 재실행이 필요 없습니다.
- **MCP 호환성.** 호스트에서 `localhost:5432`로 연결하는 데이터베이스 MCP 도구가 있다면, 공유 Postgres가 동일한 포트로 호스트에 있으므로 계속 동작합니다. 재설정이 필요 없습니다.

**트레이드오프:** Coast 인스턴스 간 데이터 격리가 없습니다. 모든 인스턴스가 동일한 데이터베이스를 읽고 씁니다. 워크플로에 인스턴스별 데이터베이스가 필요하다면 대신 `strategy = "isolated"`를 사용하는 [볼륨 전략](../concepts_and_terminology/VOLUMES.md)을 사용하거나, 공유 Postgres 안에서 인스턴스별 데이터베이스를 얻기 위해 공유 서비스에 `auto_create_db = true`를 사용하세요. 자세한 내용은 [Shared Services Coastfile reference](../coastfiles/SHARED_SERVICES.md)를 참고하세요.

**볼륨 이름이 중요합니다.** 볼륨 이름(`infra_postgres`, `infra_redis`)은 로컬에서 `docker-compose up`을 실행해 호스트에 이미 존재하는 볼륨과 일치해야 합니다. 일치하지 않으면 공유 서비스는 빈 볼륨으로 시작합니다. 이 섹션을 작성하기 전에 `docker volume ls`를 실행해 기존 볼륨 이름을 확인하세요.

## 베어 서비스

```toml
[services.vite-web]
install = "cd /workspace && yarn install --immutable 2>/dev/null || yarn install"
command = "cd /workspace && yarn workspace @acme/web run dev"
port = 3040
restart = "on-failure"
cache = ["node_modules"]

[services.vite-api]
command = "cd /workspace && yarn workspace @acme/api run dev"
port = 3036
restart = "on-failure"
```

Vite 개발 서버는 [베어 서비스](../concepts_and_terminology/BARE_SERVICES.md)로 정의됩니다 — Docker Compose 바깥에서, DinD 호스트에서 직접 실행되는 일반 프로세스입니다. 이는 [혼합 서비스 타입](../concepts_and_terminology/MIXED_SERVICE_TYPES.md) 패턴입니다.

**왜 compose 대신 베어인가요?**

주된 이유는 네트워킹입니다. Vite 개발 서버에 도달해야 하는 compose 서비스(SSR, 에셋 프록시, HMR WebSocket 연결 등)는 `host.docker.internal`을 사용해 DinD 호스트의 베어 서비스에 접근할 수 있습니다. 이는 복잡한 Docker 네트워크 설정을 피하고, 대부분의 모노레포 설정에서 `VITE_RUBY_HOST` 또는 유사한 환경 변수를 구성하는 방식과 일치합니다.

베어 서비스는 내부 컨테이너의 오버레이를 거치지 않고 바인드 마운트된 `/workspace` 파일시스템에 직접 접근합니다. 이는 Vite의 파일 워처가 변경 사항에 더 빠르게 반응한다는 뜻입니다.

**`install`과 `cache`:** `install` 필드는 서비스 시작 전에 실행되며, 매 `coast assign`마다 다시 실행됩니다. 여기서는 브랜치를 전환할 때 의존성 변경을 반영하기 위해 `yarn install`을 실행합니다. `cache` 필드는 워크트리 전환 사이에 `node_modules`를 보존하도록 Coast에 지시하여, 설치가 매번 처음부터가 아니라 증분으로 수행되게 합니다.

**`install`은 하나만:** `vite-api`에는 `install` 필드가 없다는 점에 주목하세요. yarn workspaces 모노레포에서는 루트에서 한 번 `yarn install`을 실행하면 모든 워크스페이스의 의존성이 설치됩니다. 한 서비스에만 두면 두 번 실행하는 것을 피할 수 있습니다.

## 포트와 헬스체크

```toml
[ports]
api = 3000
web = 3002
vite-web = 3040
vite-api = 3036

[healthcheck]
web = "/"
api = "/"
```

Coast가 관리하길 원하는 모든 포트는 `[ports]`에 넣습니다. 각 인스턴스는 선언된 각 포트에 대해 [동적 포트](../concepts_and_terminology/PORTS.md)(높은 범위, 항상 접근 가능)를 받습니다. 또한 [체크아웃된](../concepts_and_terminology/CHECKOUT.md) 인스턴스는 호스트에 정식 포트(선언한 번호)가 포워딩됩니다.

`[healthcheck]` 섹션은 각 포트의 상태를 Coast가 어떻게 프로브할지 알려줍니다. 헬스체크 경로가 설정된 포트에 대해 Coast는 5초마다 HTTP GET을 보내며 — 어떤 HTTP 응답이든 정상으로 간주합니다. 헬스체크 경로가 없는 포트는 TCP 연결 체크(포트가 연결을 받아들일 수 있는가?)로 폴백합니다.

이 예제에서는 Rails 웹 서버가 HTML 페이지를 제공하므로 `/`에서 HTTP 헬스체크를 받습니다. Vite 개발 서버는 헬스체크 경로를 지정하지 않습니다 — 의미 있는 루트 페이지를 제공하지 않으며, 연결을 수락하는지만 확인하면 TCP 체크로 충분하기 때문입니다.

헬스체크 상태는 [Coastguard](../concepts_and_terminology/COASTGUARD.md) UI와 `coast ports`를 통해 확인할 수 있습니다.

## 볼륨

```toml
[volumes.bundle]
strategy = "shared"
service = "api-rails"
mount = "/usr/local/bundle"

[volumes.api_rails_cache]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/tmp/cache"

[volumes.api_assets]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/public/assets"

[volumes.web_rails_cache]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/tmp/cache"

[volumes.web_assets]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/public/assets"
```

여기의 모든 볼륨은 `strategy = "shared"`를 사용합니다. 이는 단일 Docker 볼륨이 모든 Coast 인스턴스에 걸쳐 공유된다는 뜻입니다. 이는 **캐시와 빌드 아티팩트**에 올바른 선택입니다 — 동시 쓰기가 안전하고, 인스턴스별로 복제하면 디스크 공간을 낭비하고 시작이 느려지는 것들입니다:

- **`bundle`** — Ruby gem 캐시. gem은 브랜치 간에 동일합니다. 공유하면 각 Coast 인스턴스마다 전체 번들을 다시 다운로드할 필요가 없습니다.
- **`*_rails_cache`** — Rails 파일 기반 캐시. 개발 속도를 높이지만 중요 데이터는 아닙니다 — 어떤 인스턴스든 재생성할 수 있습니다.
- **`*_assets`** — 컴파일된 에셋. 캐시와 같은 이유입니다.

**왜 데이터베이스에는 shared를 쓰지 않나요?** Coast는 데이터베이스류 서비스에 붙은 볼륨에 `strategy = "shared"`를 사용하면 경고를 출력합니다. 여러 Postgres 프로세스가 동일한 데이터 디렉터리에 쓰면 손상이 발생합니다. 데이터베이스의 경우 [공유 서비스](../coastfiles/SHARED_SERVICES.md)(이 레시피처럼 호스트에 Postgres 1개) 또는 `strategy = "isolated"`(각 Coast가 자신의 볼륨을 가짐)을 사용하세요. 전체 의사결정 매트릭스는 [Volume Topology](../concepts_and_terminology/VOLUMES.md) 페이지를 참고하세요.

## Assign 전략

```toml
[assign]
default = "none"
exclude_paths = [
    ".coasts",
    ".yarn",
    ".github",
    "docs",
    "scripts",
    "cli",
    "deploy",
    "tools",
    "mobile",
    "extensions",
]

[assign.services]
web-rails = "hot"
web-ssr = "hot"
web-sidekiq = "restart"
api-rails = "hot"
api-sidekiq = "restart"

[assign.rebuild_triggers]
web-rails = ["infra/Dockerfile", "web/Gemfile", "web/Gemfile.lock", "web/package.json"]
api-rails = ["infra/Dockerfile", "api/Gemfile", "api/Gemfile.lock", "api/package.json"]
```

`[assign]` 섹션은 `coast assign`으로 Coast 인스턴스를 다른 워크트리로 전환할 때 각 서비스에 어떤 일이 일어나는지 제어합니다. 이를 제대로 설정하는 것이 5초 브랜치 전환과 60초 브랜치 전환의 차이를 만듭니다.

### `default = "none"`

기본값을 `"none"`으로 설정하면 `[assign.services]`에 명시적으로 나열되지 않은 서비스는 브랜치 전환 시 그대로 둡니다. 이는 데이터베이스와 캐시에 매우 중요합니다 — Postgres, Redis, 인프라 서비스는 브랜치 간에 바뀌지 않으므로 재시작은 낭비입니다.

### 서비스별 전략

| Service | Strategy | Why |
|---|---|---|
| `web-rails`, `web-ssr`, `api-rails` | `hot` | 이들은 파일 워처가 있는 개발 서버를 실행합니다. [파일시스템 리마운트](../concepts_and_terminology/FILESYSTEM.md)가 `/workspace` 아래 코드를 교체하면 워처가 변경을 자동으로 감지합니다. 컨테이너 재시작이 필요 없습니다. |
| `web-sidekiq`, `api-sidekiq` | `restart` | 백그라운드 워커는 시작 시 코드를 로드하며 파일 변경을 감시하지 않습니다. 새 브랜치의 코드를 반영하려면 컨테이너 재시작이 필요합니다. |

실제로 실행 중인 서비스만 나열하세요. `COMPOSE_PROFILES`가 일부 서비스만 시작한다면 비활성 서비스를 나열하지 마세요 — Coast는 나열된 모든 서비스에 대해 assign 전략을 평가하며, 실행 중이 아닌 서비스를 재시작하는 것은 낭비입니다. 자세한 내용은 [Performance Optimizations](../concepts_and_terminology/PERFORMANCE_OPTIMIZATIONS.md)를 참고하세요.

### `exclude_paths`

이는 대규모 모노레포에서 가장 영향이 큰 최적화입니다. 이는 매 assign마다 실행되는 gitignored 파일 동기화(rsync)와 `git ls-files` diff 동안 Coast가 전체 디렉터리 트리를 건너뛰도록 지시합니다.

목표는 Coast 서비스에 필요하지 않은 모든 것을 제외하는 것입니다. 30,000개 파일이 있는 모노레포에서 위에 나열한 디렉터리들은 실행 중인 서비스와 무관한 8,000개 이상의 파일을 차지할 수 있습니다. 이를 제외하면 브랜치 전환마다 그만큼의 파일 stat을 줄일 수 있습니다.

무엇을 제외할지 찾기 위해 레포를 프로파일링하세요:

```bash
git ls-files | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

실행 중인 서비스에 마운트되는 소스 코드 또는 해당 서비스가 import하는 공유 라이브러리가 들어있는 디렉터리는 유지하세요. 그 외는 모두 제외하세요 — 문서, CI 설정, 도구, 다른 팀의 앱, 모바일 클라이언트, CLI 도구, 그리고 `.yarn` 같은 벤더드 캐시 등입니다.

### `rebuild_triggers`

트리거가 없으면 `strategy = "rebuild"`인 서비스는 브랜치 전환 때마다 Docker 이미지를 리빌드합니다 — 이미지에 영향을 주는 것이 아무것도 바뀌지 않았더라도 말입니다. `[assign.rebuild_triggers]` 섹션은 특정 파일에 따라 리빌드를 제한합니다.

이 레시피에서 Rails 서비스는 보통 `"hot"`(재시작도 없음)을 사용합니다. 하지만 누군가 Dockerfile이나 Gemfile을 변경하면 `rebuild_triggers`가 동작해 전체 이미지 리빌드를 강제합니다. 트리거 파일이 변경되지 않았다면 Coast는 리빌드를 완전히 건너뜁니다. 이는 일상적인 코드 변경에서는 비용이 큰 이미지 빌드를 피하면서도, 인프라 수준 변경은 놓치지 않게 해줍니다.

## Secrets와 Inject

```toml
[secrets.compose_profiles]
extractor = "command"
run = "echo api,web"
inject = "env:COMPOSE_PROFILES"

[secrets.uid]
extractor = "command"
run = "id -u"
inject = "env:UID"

[secrets.gid]
extractor = "command"
run = "id -g"
inject = "env:GID"

[inject]
env = ["USER", "BUNDLE_GEMS__CONTRIBSYS__COM"]
```

`[secrets]` 섹션은 빌드 시점에 값을 추출해 Coast 인스턴스에 환경 변수로 주입합니다.

- **`compose_profiles`**는 어떤 Docker Compose 프로파일을 시작할지 제어합니다. 이는 compose 파일에 정의된 모든 서비스를 실행하는 대신 `api`와 `web` 프로파일만 실행하도록 Coast를 제한하는 방법입니다. 빌드 전에 호스트에서 `export COMPOSE_PROFILES=api,web,portal`로 덮어써서 어떤 서비스가 시작될지 변경할 수 있습니다.
- **`uid` / `gid`**는 호스트 사용자의 UID와 GID를 컨테이너로 전달합니다. 이는 호스트와 컨테이너 사이에서 파일 소유권이 일치해야 하는 Docker 설정에서 흔히 사용됩니다.

`[inject]` 섹션은 더 단순합니다 — 기존 호스트 환경 변수를 런타임에 Coast 컨테이너로 전달합니다. gem 서버 토큰(`BUNDLE_GEMS__CONTRIBSYS__COM`) 같은 민감한 자격 증명은 호스트에 남아 있으며, 어떤 설정 파일에도 기록되지 않은 채로 포워딩됩니다.

시크릿 extractor와 injection 대상에 대한 전체 레퍼런스는 [Secrets](../coastfiles/SECRETS.md)를 참고하세요.

## 이 레시피 적용하기

**다른 언어 스택:** Rails 전용 볼륨(bundle, rails cache, assets)을 해당 스택의 동등한 것으로 교체하세요 — Go 모듈 캐시(`/go/pkg/mod`), npm 캐시, pip 캐시 등. 인스턴스 간 공유해도 안전한 캐시라면 전략은 `"shared"`로 유지합니다.

**앱이 더 적은 경우:** 모노레포에 앱이 하나뿐이라면 추가 볼륨 엔트리를 제거하고, `[assign.services]`를 본인 서비스만 나열하도록 단순화하세요. 공유 서비스와 베어 서비스 패턴은 여전히 적용됩니다.

**인스턴스별 데이터베이스:** Coast 인스턴스 간 데이터 격리가 필요하다면 `[shared_services.db]`를 compose 내부 Postgres로 교체하고, `strategy = "isolated"`인 `[volumes]` 엔트리를 추가하세요. 각 인스턴스는 자체 데이터베이스 볼륨을 갖습니다. `snapshot_source`를 사용해 호스트 볼륨에서 시드할 수 있습니다 — [Volumes Coastfile reference](../coastfiles/VOLUMES.md)를 참고하세요.

**베어 서비스가 없는 경우:** 프론트엔드가 완전히 컨테이너화되어 있고 `host.docker.internal`을 통해 접근될 필요가 없다면 `[services.*]` 섹션과 `[coast.setup]`을 제거하세요. 모든 것이 compose를 통해 실행됩니다.
