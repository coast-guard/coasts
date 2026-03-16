# 프로젝트 및 설정

`[coast]` 섹션은 Coastfile에서 유일하게 필수인 섹션입니다. 이 섹션은 프로젝트를 식별하고 Coast 컨테이너가 생성되는 방식을 구성합니다. 선택 사항인 `[coast.setup]` 하위 섹션을 사용하면 빌드 시점에 컨테이너 내부에 패키지를 설치하고 명령을 실행할 수 있습니다.

## `[coast]`

### `name` (필수)

프로젝트의 고유 식별자입니다. 컨테이너 이름, 볼륨 이름, 상태 추적, CLI 출력에 사용됩니다.

```toml
[coast]
name = "my-app"
```

### `compose`

Docker Compose 파일의 경로입니다. 상대 경로는 프로젝트 루트(Coastfile이 들어 있는 디렉터리, 또는 `root`가 설정된 경우 그 디렉터리)를 기준으로 해석됩니다.

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
```

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
```

생략하면 Coast 컨테이너는 `docker compose up`을 실행하지 않고 시작됩니다. [bare services](SERVICES.md)를 사용하거나 `coast exec`를 통해 컨테이너와 직접 상호작용할 수 있습니다.

같은 Coastfile에서 `compose`와 `[services]`를 동시에 설정할 수는 없습니다.

### `runtime`

사용할 컨테이너 런타임입니다. 기본값은 `"dind"`(Docker-in-Docker)입니다.

- `"dind"` — `--privileged`를 사용하는 Docker-in-Docker. 프로덕션에서 검증된 유일한 런타임입니다. [Runtimes and Services](../concepts_and_terminology/RUNTIMES_AND_SERVICES.md)를 참고하세요.
- `"sysbox"` — privileged 모드 대신 Sysbox 런타임을 사용합니다. Sysbox가 설치되어 있어야 합니다.
- `"podman"` — 내부 컨테이너 런타임으로 Podman을 사용합니다.

```toml
[coast]
name = "my-app"
runtime = "dind"
```

### `root`

프로젝트 루트 디렉터리를 재정의합니다. 기본적으로 프로젝트 루트는 Coastfile이 들어 있는 디렉터리입니다. 상대 경로는 Coastfile의 디렉터리를 기준으로 해석되며, 절대 경로는 그대로 사용됩니다.

```toml
[coast]
name = "my-app"
root = "../my-project"
```

이는 드문 경우입니다. 대부분의 프로젝트는 Coastfile을 실제 프로젝트 루트에 둡니다.

### `worktree_dir`

git worktree가 위치하는 디렉터리들입니다. 단일 문자열 또는 문자열 배열을 받습니다. 기본값은 `".worktrees"`입니다.

```toml
# Single directory
worktree_dir = ".worktrees"

# Multiple directories, including an external one
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

상대 경로는 프로젝트 루트를 기준으로 해석됩니다. `~/` 또는 `/`로 시작하는 경로는 **외부** 디렉터리로 취급되며, Coast는 컨테이너가 이에 접근할 수 있도록 별도의 바인드 마운트를 추가합니다. 이것이 Codex처럼 프로젝트 루트 밖에 worktree를 생성하는 도구와 통합하는 방법입니다.

런타임에 Coast는 기존 git worktree(`git worktree list`를 통해)에서 worktree 디렉터리를 자동 감지하고, 모든 worktree가 하나의 디렉터리에 일치하면 구성된 기본값보다 이를 우선합니다.

외부 디렉터리 동작, 프로젝트 필터링, 예제를 포함한 전체 참조는 [Worktree Directories](WORKTREE_DIR.md)를 참고하세요.

### `default_worktree_dir`

**새로운** worktree를 생성할 때 사용할 디렉터리입니다. 기본값은 `worktree_dir`의 첫 번째 항목입니다. `worktree_dir`가 배열일 때만 관련이 있습니다.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
default_worktree_dir = ".worktrees"
```

### `autostart`

`coast run`으로 Coast 인스턴스를 생성할 때 `docker compose up`(또는 bare services 시작)을 자동으로 실행할지 여부입니다. 기본값은 `true`입니다.

컨테이너는 실행 중이되 서비스를 수동으로 시작하고 싶을 때 `false`로 설정하세요 — 필요할 때 테스트를 실행하는 테스트 러너 변형에 유용합니다.

```toml
[coast]
name = "my-app"
extends = "Coastfile"
autostart = false
```

### `primary_port`

빠른 링크와 서브도메인 라우팅에 사용할 `[ports]` 섹션의 포트를 지정합니다. 값은 `[ports]`에 정의된 키와 일치해야 합니다.

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

이를 통해 서브도메인 라우팅과 URL 템플릿이 활성화되는 방식은 [Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md)를 참고하세요.

## `[coast.setup]`

Coast 컨테이너 자체를 커스터마이즈합니다 — 도구 설치, 빌드 단계 실행, 설정 파일 구체화 등을 포함합니다. `[coast.setup]`의 모든 항목은 DinD 컨테이너 내부에서 실행됩니다(compose 서비스 내부가 아님).

### `packages`

설치할 APK 패키지입니다. 기본 DinD 이미지가 Alpine 기반이므로 Alpine Linux 패키지입니다.

```toml
[coast.setup]
packages = ["nodejs", "npm", "git", "curl"]
```

### `run`

빌드 중 순서대로 실행되는 셸 명령입니다. APK 패키지로 제공되지 않는 도구를 설치할 때 사용하세요.

```toml
[coast.setup]
packages = ["nodejs", "npm", "python3", "wget", "bash", "ca-certificates"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]
```

### `[[coast.setup.files]]`

컨테이너 내부에 생성할 파일입니다. 각 항목은 `path`(필수, 반드시 절대 경로), `content`(필수), 선택 사항인 `mode`(3~4자리 8진수 문자열)를 가집니다.

```toml
[coast.setup]
packages = ["nodejs", "npm"]
run = ["mkdir -p /app/config"]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```

파일 항목에 대한 검증 규칙:

- `path`는 절대 경로여야 합니다(`/`로 시작)
- `path`는 `..` 구성요소를 포함하면 안 됩니다
- `path`는 `/`로 끝나면 안 됩니다
- `mode`는 3자리 또는 4자리 8진수 문자열이어야 합니다(예: `"600"`, `"0644"`)

## 전체 예시

Go 및 Node.js 개발을 위해 설정된 Coast 컨테이너:

```toml
[coast]
name = "my-fullstack-app"
compose = "./docker-compose.yml"
runtime = "dind"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
primary_port = "web"

[coast.setup]
packages = ["nodejs", "npm", "python3", "make", "curl", "git", "bash", "ca-certificates", "wget", "gcc", "musl-dev"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz && ln -s /usr/local/go/bin/go /usr/local/bin/go",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
    "pip3 install --break-system-packages pgcli",
]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```
