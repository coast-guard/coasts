# Coastfile 유형

하나의 프로젝트는 서로 다른 사용 사례를 위해 여러 Coastfile을 가질 수 있습니다. 각 변형을 "유형(type)"이라고 부릅니다. 유형을 사용하면 공통 기반을 공유하면서도 어떤 서비스가 실행되는지, 볼륨을 어떻게 처리하는지, 또는 서비스가 자동 시작되는지 여부가 다른 구성을 조합할 수 있습니다.

## 유형이 동작하는 방식

이름 규칙은 기본은 `Coastfile`, 변형은 `Coastfile.{type}`입니다. 점 뒤의 접미사가 유형 이름이 됩니다:

- `Coastfile` -- 기본 유형
- `Coastfile.test` -- 테스트 유형
- `Coastfile.snap` -- 스냅샷 유형
- `Coastfile.light` -- 경량 유형

`--type`으로 유형이 지정된 Coast를 빌드하고 실행합니다:

```bash
coast build --type test
coast run test-1 --type test
coast exec test-1 -- go test ./...
```

## extends

유형이 지정된 Coastfile은 `extends`를 통해 부모로부터 상속합니다. 부모의 모든 항목이 병합됩니다. 자식은 오버라이드하거나 추가하는 것만 지정하면 됩니다.

```toml
[coast]
extends = "Coastfile"
```

이를 통해 각 변형마다 전체 구성을 중복하지 않아도 됩니다. 자식은 부모로부터 모든 [ports](PORTS.md), [secrets](SECRETS.md), [volumes](VOLUMES.md), [shared services](SHARED_SERVICES.md), [assign strategies](ASSIGN.md), 설정 명령, 그리고 [MCP](MCP_SERVERS.md) 구성을 상속합니다. 자식이 정의한 모든 것은 부모보다 우선합니다.

## [unset]

부모로부터 상속된 특정 항목을 이름으로 제거합니다. `ports`, `shared_services`, `secrets`, `volumes`를 unset할 수 있습니다.

```toml
[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]
```

이것이 테스트 변형에서 공유 서비스를 제거하는 방식입니다(그래서 데이터베이스가 격리된 볼륨을 가진 Coast 내부에서 실행됨). 또한 필요하지 않은 포트도 제거합니다.

## [omit]

빌드에서 compose 서비스를 완전히 제거합니다. omit된 서비스는 compose 파일에서 제거되며 Coast 내부에서 전혀 실행되지 않습니다.

```toml
[omit]
services = ["redis", "backend", "mailhog", "web"]
```

변형의 목적과 무관한 서비스를 제외할 때 사용합니다. 테스트 변형은 데이터베이스, 마이그레이션, 테스트 러너만 남길 수 있습니다.

## autostart

Coast가 시작될 때 `docker compose up`을 자동으로 실행할지 제어합니다. 기본값은 `true`입니다.

```toml
[coast]
extends = "Coastfile"
autostart = false
```

전체 스택을 올리는 대신 특정 명령을 수동으로 실행하고 싶은 변형에는 `autostart = false`를 설정하세요. 이는 테스트 러너에서 흔합니다 -- Coast를 만든 다음 [`coast exec`](EXEC_AND_DOCKER.md)로 개별 테스트 스위트를 실행합니다.

## 일반적인 패턴

### 테스트 변형

테스트 실행에 필요한 것만 유지하는 `Coastfile.test`:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]

[omit]
services = ["redis", "backend", "mailhog", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[assign]
default = "none"
[assign.services]
test-runner = "rebuild"
migrations = "rebuild"
```

각 테스트 Coast는 자체의 깨끗한 데이터베이스를 갖습니다. 테스트는 내부 compose 네트워크를 통해 서비스와 통신하므로 어떤 포트도 노출되지 않습니다. `autostart = false`는 `coast exec`로 테스트 실행을 수동으로 트리거한다는 의미입니다.

### 스냅샷 변형

호스트의 기존 데이터베이스 볼륨 복사본으로 각 Coast를 시드하는 `Coastfile.snap`:

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis"]

[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "my_project_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "my_project_redis_data"
service = "redis"
mount = "/data"
```

공유 서비스를 unset하여 데이터베이스가 각 Coast 내부에서 실행되도록 합니다. `snapshot_source`는 빌드 시점에 기존 호스트 볼륨에서 격리된 볼륨을 시드합니다. 생성 이후에는 각 인스턴스의 데이터가 서로 독립적으로 분기됩니다.

### 경량 변형

특정 워크플로를 위해 프로젝트를 최소 구성으로 줄이는 `Coastfile.light` -- 예를 들어 빠른 반복을 위해 백엔드 서비스와 해당 데이터베이스만 남길 수 있습니다.

## 독립적인 빌드 풀

각 유형은 자체 `latest-{type}` 심볼릭 링크와 자체 5개 빌드 자동 프루닝 풀을 갖습니다:

```bash
coast build              # latest 업데이트, 기본 빌드 프루닝
coast build --type test  # latest-test 업데이트, test 빌드 프루닝
coast build --type snap  # latest-snap 업데이트, snap 빌드 프루닝
```

`test` 유형을 빌드해도 `default` 또는 `snap` 빌드에는 영향을 주지 않습니다. 프루닝은 유형별로 완전히 독립적입니다.

## 유형이 지정된 Coast 실행

`--type`으로 생성된 인스턴스는 해당 유형으로 태그됩니다. 같은 프로젝트에 대해 서로 다른 유형의 인스턴스를 동시에 실행할 수 있습니다:

```bash
coast run dev-1                    # 기본 유형
coast run test-1 --type test       # 테스트 유형
coast run snapshot-1 --type snap   # 스냅샷 유형

coast ls
# 세 개 모두 표시되며, 각각 고유한 유형, 포트, 볼륨 전략을 가짐
```

이렇게 하면 동일한 프로젝트에 대해 전체 개발 환경을 실행하면서도 격리된 테스트 러너와 스냅샷으로 시드된 인스턴스를 함께, 동시에 실행할 수 있습니다.
