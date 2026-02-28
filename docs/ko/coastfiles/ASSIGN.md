# Assign

`[assign]` 섹션은 `coast assign`로 브랜치를 전환할 때 Coast 인스턴스 내부의 서비스에 어떤 일이 일어나는지를 제어합니다. 각 서비스는 전체 재빌드, 재시작, 핫 리로드, 또는 아무 작업도 하지 않음 중 무엇이 필요한지에 따라 서로 다른 전략으로 구성할 수 있습니다.

런타임에서 `coast assign` 및 `coast unassign`이 어떻게 동작하는지에 대해서는 [Assign](../concepts_and_terminology/ASSIGN.md)를 참고하세요.

## `[assign]`

### `default`

브랜치 전환 시 모든 서비스에 적용되는 기본 동작입니다. 전체 `[assign]` 섹션이 생략되면 기본값은 `"restart"`입니다.

- **`"none"`** — 아무 것도 하지 않습니다. 서비스는 현재 상태로 계속 실행됩니다. 코드에 의존하지 않는 데이터베이스와 캐시에 적합합니다.
- **`"hot"`** — 코드는 [filesystem](../concepts_and_terminology/FILESYSTEM.md)을 통해 이미 라이브 마운트되어 있으므로, 서비스가 변경 사항을 자동으로 반영합니다(예: 파일 감시자 또는 핫 리로드를 통해). 컨테이너 재시작이 필요 없습니다.
- **`"restart"`** — 서비스 컨테이너를 재시작합니다. 서비스가 시작 시 코드를 읽지만 전체 이미지 재빌드는 필요하지 않을 때 사용하세요.
- **`"rebuild"`** — 서비스의 Docker 이미지를 재빌드하고 재시작합니다. Dockerfile에서 `COPY` 또는 `ADD`로 코드가 이미지에 포함(bake)되는 경우 필요합니다.

```toml
[assign]
default = "none"
```

### `[assign.services]`

서비스별 재정의입니다. 각 키는 compose 서비스 이름이고, 값은 위 네 가지 동작 중 하나입니다.

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

이를 통해 데이터베이스와 캐시는 그대로 두고(기본값으로 `"none"`), 변경된 코드에 의존하는 서비스만 재빌드하거나 재시작할 수 있습니다.

### `[assign.rebuild_triggers]`

기본 동작이 더 가벼운 경우라도, 특정 서비스에 대해 재빌드를 강제하는 파일 패턴입니다. 각 키는 서비스 이름이며, 값은 파일 경로 또는 패턴의 목록입니다.

```toml
[assign]
default = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json", "package-lock.json"]
```

### `exclude_paths`

`coast assign` 중 worktree 동기화에서 제외할 경로 목록입니다. 큰 모노레포에서 특정 디렉터리가 Coast에서 실행 중인 서비스와 무관하여, 그렇지 않으면 assign 작업을 느리게 만들 수 있을 때 유용합니다.

```toml
[assign]
default = "none"
exclude_paths = ["apps/ide", "apps/extension", "apps/ide-extension"]

[assign.services]
backend = "hot"
web = "hot"
```

## Examples

### 앱을 재빌드하고, 나머지는 모두 그대로 두기

app 서비스가 코드를 Docker 이미지에 포함(bake)하지만 데이터베이스는 코드 변경과 독립적일 때:

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

### 프론트엔드와 백엔드 핫 리로드

두 서비스 모두 파일 감시자(예: Next.js dev server, Go air, nodemon)를 사용하고 코드가 라이브 마운트되어 있을 때:

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

### 트리거를 사용한 서비스별 재빌드

API 서비스는 보통 재시작만 하지만, `Dockerfile` 또는 `package.json`이 변경되면 재빌드합니다:

```toml
[assign]
default = "none"

[assign.services]
api = "restart"
worker = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json"]
```

### 모든 것을 전체 재빌드

모든 서비스가 코드를 이미지에 포함(bake)할 때:

```toml
[assign]
default = "rebuild"
```
