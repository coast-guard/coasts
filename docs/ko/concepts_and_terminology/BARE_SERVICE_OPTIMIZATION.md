# 베어 서비스 최적화

[베어 서비스](BARE_SERVICES.md)는 Coast 컨테이너 내부에서 일반 프로세스로 실행됩니다. Docker 레이어나 이미지 캐시가 없기 때문에, 시작 속도와 브랜치 전환 성능은 `install` 명령, 캐싱, assign 전략을 어떻게 구성하느냐에 따라 달라집니다.

## 빠른 Install 명령

`install` 필드는 서비스가 시작되기 전에 실행되며, 모든 `coast assign` 때 다시 실행됩니다. `install`이 조건 없이 `make` 또는 `yarn install`을 실행하면, 변경된 것이 없더라도 브랜치를 전환할 때마다 전체 설치 비용을 치르게 됩니다.

**가능하면 작업을 건너뛰도록 조건 검사를 사용하세요:**

```toml
[services.web]
install = "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)"
command = "cd /workspace && yarn dev:web"
```

`test -f` 가드는 `node_modules`가 이미 존재하면 설치를 건너뜁니다. 첫 실행 시 또는 캐시 미스 이후에는 전체 설치를 실행합니다. 이후 의존성이 변경되지 않은 assign에서는 즉시 완료됩니다.

컴파일된 바이너리의 경우, 출력물이 존재하는지 확인하세요:

```toml
[services.zoekt]
install = "cd /workspace && (test -f bin/zoekt-webserver || make zoekt)"
command = "cd /workspace && ./bin/zoekt-webserver -index .sourcebot/index -rpc"
```

## 워크트리 간 캐시 디렉터리 유지

Coast가 베어 서비스 인스턴스를 새 워크트리로 전환할 때, `/workspace` 마운트는 다른 디렉터리로 변경됩니다. `node_modules`나 컴파일된 바이너리 같은 빌드 산출물은 이전 워크트리에 남겨집니다. `cache` 필드는 전환 간에 지정한 디렉터리를 Coast가 유지하도록 지시합니다:

```toml
[services.web]
install = "cd /workspace && yarn install"
command = "cd /workspace && yarn dev"
cache = ["node_modules"]

[services.api]
install = "cd /workspace && make build"
command = "cd /workspace && ./bin/api-server"
cache = ["bin"]
```

캐시된 디렉터리는 워크트리 재마운트 전에 백업되고 이후 복원됩니다. 이는 `yarn install`이 처음부터 다시 실행되는 대신 증분 방식으로 실행된다는 뜻이며, 컴파일된 바이너리도 브랜치 전환 후 유지됩니다.

## private_paths로 인스턴스별 디렉터리 격리

일부 도구는 워크스페이스 안에 프로세스별 상태를 담는 디렉터리를 생성합니다: 잠금 파일, 빌드 캐시, 또는 PID 파일 등입니다. 여러 Coast 인스턴스가 같은 워크스페이스를 공유하면(같은 브랜치, 워크트리 없음), 이러한 디렉터리가 충돌합니다.

대표적인 예는 Next.js이며, 시작 시 `.next/dev/lock`에 잠금을 겁니다. 두 번째 Coast 인스턴스는 이 잠금을 보고 시작을 거부합니다.

`private_paths`는 지정한 경로에 대해 각 인스턴스에 고유한 격리 디렉터리를 제공합니다:

```toml
[coast]
name = "my-app"
private_paths = ["packages/web/.next"]
```

각 인스턴스는 해당 경로에 인스턴스별 오버레이 마운트를 갖게 됩니다. 잠금 파일, 빌드 캐시, Turbopack 상태가 완전히 격리됩니다. 코드 변경은 필요 없습니다.

동시에 여러 인스턴스가 같은 파일에 쓰는 것이 문제를 일으키는 디렉터리라면 `private_paths`를 사용하세요: `.next`, `.turbo`, `.parcel-cache`, PID 파일, 또는 SQLite 데이터베이스 등입니다.

## 공유 서비스에 연결하기

데이터베이스나 캐시를 위해 [공유 서비스](SHARED_SERVICES.md)를 사용할 때, 공유 컨테이너는 Coast 내부가 아니라 호스트 Docker 데몬에서 실행됩니다. Coast 내부에서 실행되는 베어 서비스는 `localhost`를 통해 이들에 접근할 수 없습니다.

대신 `host.docker.internal`을 사용하세요:

```toml
[services.web]
command = "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

또한 [secrets](../coastfiles/SECRETS.md)를 사용해 연결 문자열을 환경 변수로 주입할 수도 있습니다:

```toml
[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"
```

Coast 내부의 compose 서비스에는 이 문제가 없습니다. Coast는 compose 컨테이너에 대해 공유 서비스 호스트명을 브리지 네트워크를 통해 자동으로 라우팅합니다. 이는 베어 서비스에만 영향을 줍니다.

## 인라인 환경 변수

베어 서비스 명령은 `.env` 파일, secrets, inject를 통해 설정된 것을 포함하여 Coast 컨테이너의 환경 변수를 상속합니다. 하지만 때로는 공유 설정 파일을 변경하지 않고 특정 서비스 하나에 대해서만 특정 변수를 재정의해야 할 수 있습니다.

명령 앞에 인라인 할당을 붙이세요:

```toml
[services.web]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

인라인 변수는 다른 모든 것보다 우선합니다. 이는 다음과 같은 경우에 유용합니다:

- 인증 리디렉션이 체크아웃되지 않은 인스턴스에서도 작동하도록 `AUTH_URL`을 [동적 포트](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md)로 설정
- `DATABASE_URL`을 `host.docker.internal`을 통해 공유 서비스를 가리키도록 재정의
- 워크스페이스의 공유 `.env` 파일을 수정하지 않고 서비스별 플래그 설정

## 베어 서비스용 Assign 전략

각 서비스가 코드 변경을 반영하는 방식에 따라 적절한 [assign 전략](../coastfiles/ASSIGN.md)을 선택하세요:

| Strategy | When to use | Examples |
|---|---|---|
| `hot` | 워크트리 재마운트 후 변경을 자동으로 감지하는 파일 감시자가 서비스에 있는 경우 | Next.js (HMR), Vite, webpack, nodemon, tsc --watch |
| `restart` | 서비스가 시작 시 코드를 로드하고 변경을 감시하지 않는 경우 | 컴파일된 Go 바이너리, Rails, Java 서버 |
| `none` | 서비스가 워크스페이스 코드에 의존하지 않거나 별도의 인덱스를 사용하는 경우 | 데이터베이스 서버, Redis, 검색 인덱스 |

```toml
[assign]
default = "none"

[assign.services]
web = "hot"
backend = "hot"
zoekt = "none"
```

기본값을 `none`으로 설정하면 브랜치 전환 시 인프라 서비스는 절대 건드리지 않게 됩니다. 코드 변경에 관심 있는 서비스만 재시작되거나 핫 리로드에 의존하게 됩니다.

## 참고

- [베어 서비스](BARE_SERVICES.md) - 베어 서비스 전체 참조
- [성능 최적화](PERFORMANCE_OPTIMIZATIONS.md) - `exclude_paths` 및 `rebuild_triggers`를 포함한 일반적인 성능 튜닝
- [동적 포트 환경 변수](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - 명령에서 `WEB_DYNAMIC_PORT` 및 관련 변수 사용
