# Coastfile 타입

하나의 프로젝트는 서로 다른 사용 사례를 위해 여러 Coastfile을 가질 수 있습니다. 각 변형은 "타입"이라고 합니다. 타입을 사용하면 공통 기반을 공유하지만 어떤 서비스가 실행되는지, 볼륨이 어떻게 처리되는지, 또는 서비스가 자동 시작되는지 여부가 다른 구성을 조합할 수 있습니다.

## 타입 동작 방식

명명 규칙은 기본값에 대해 `Coastfile`, 변형에 대해 `Coastfile.{type}`입니다. 점 뒤의 접미사가 타입 이름이 됩니다:

- `Coastfile` -- 기본 타입
- `Coastfile.test` -- 테스트 타입
- `Coastfile.snap` -- 스냅샷 타입
- `Coastfile.light` -- 경량 타입

모든 Coastfile은 에디터 구문 강조를 위해 선택적으로 `.toml` 확장자를 가질 수 있습니다. 타입을 도출하기 전에 `.toml` 접미사는 제거되므로, 다음 쌍은 서로 동일합니다:

- `Coastfile.toml` = `Coastfile` (기본 타입)
- `Coastfile.test.toml` = `Coastfile.test` (테스트 타입)
- `Coastfile.light.toml` = `Coastfile.light` (경량 타입)

**우선순위 규칙:** 두 형태가 모두 존재하는 경우(예: `Coastfile` 및 `Coastfile.toml`, 또는 `Coastfile.light` 및 `Coastfile.light.toml`), `.toml` 변형이 우선합니다.

**예약된 타입 이름:** `"default"` 및 `"toml"`은 타입 이름으로 사용할 수 없습니다. `Coastfile.default` 및 `Coastfile.toml`(타입 접미사로서, 즉 문자 그대로 `Coastfile.toml.toml`이라는 이름의 파일을 의미함)은 거부됩니다.

타입이 지정된 Coast는 `--type`으로 빌드하고 실행합니다:

```bash
coast build --type test
coast run test-1 --type test
coast exec test-1 -- go test ./...
```

## extends

타입이 지정된 Coastfile은 `extends`를 통해 부모로부터 상속합니다. 부모의 모든 내용이 병합됩니다. 자식은 자신이 재정의하거나 추가하는 내용만 지정하면 됩니다.

```toml
[coast]
extends = "Coastfile"
```

이렇게 하면 각 변형마다 전체 구성을 중복 작성하는 일을 피할 수 있습니다. 자식은 부모로부터 모든 [포트](PORTS.md), [시크릿](SECRETS.md), [볼륨](VOLUMES.md), [공유 서비스](SHARED_SERVICES.md), [할당 전략](ASSIGN.md), 설정 명령, 그리고 [MCP](MCP_SERVERS.md) 구성을 상속합니다. 자식이 정의한 항목은 무엇이든 부모보다 우선합니다.

## [unset]

부모로부터 상속된 특정 항목을 이름으로 제거합니다. `ports`, `shared_services`, `secrets`, `volumes`를 unset할 수 있습니다.

```toml
[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]
```

이것이 테스트 변형에서 공유 서비스를 제거하는 방법입니다(따라서 데이터베이스가 격리된 볼륨과 함께 Coast 내부에서 실행됨). 또한 필요하지 않은 포트도 제거합니다.

## [omit]

빌드에서 compose 서비스를 완전히 제거합니다. 생략된 서비스는 compose 파일에서 제거되며 Coast 내부에서 전혀 실행되지 않습니다.

```toml
[omit]
services = ["redis", "backend", "mailhog", "web"]
```

변형의 목적과 관련이 없는 서비스를 제외할 때 이것을 사용합니다. 테스트 변형은 데이터베이스, 마이그레이션, 테스트 러너만 남길 수 있습니다.

## autostart

Coast가 시작될 때 `docker compose up`를 자동으로 실행할지 제어합니다. 기본값은 `true`입니다.

```toml
[coast]
extends = "Coastfile"
autostart = false
```

전체 스택을 올리는 대신 특정 명령을 수동으로 실행하려는 변형에는 `autostart = false`를 설정하세요. 이는 테스트 러너에서 일반적입니다 -- Coast를 생성한 다음 [`coast exec`](EXEC_AND_DOCKER.md)을 사용하여 개별 테스트 스위트를 실행합니다.

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

각 테스트 Coast는 자체적으로 깨끗한 데이터베이스를 갖습니다. 테스트는 내부 compose 네트워크를 통해 서비스와 통신하므로 어떤 포트도 노출되지 않습니다. `autostart = false`는 `coast exec`로 테스트 실행을 수동으로 트리거한다는 뜻입니다.

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

공유 서비스는 unset되어 각 Coast 내부에서 데이터베이스가 실행됩니다. `snapshot_source`는 빌드 시점에 기존 호스트 볼륨으로부터 격리된 볼륨을 시드합니다. 생성 후에는 각 인스턴스의 데이터가 독립적으로 분기됩니다.

### 경량 변형

특정 워크플로를 위해 프로젝트를 최소 구성으로 줄이는 `Coastfile.light` -- 빠른 반복 작업을 위해 아마도 백엔드 서비스와 해당 데이터베이스만 남기는 형태입니다.

## 독립적인 빌드 풀

각 타입은 자체 `latest-{type}` 심볼릭 링크와 자체 5-빌드 자동 정리 풀을 가집니다:

```bash
coast build              # latest 업데이트, 기본 빌드 정리
coast build --type test  # latest-test 업데이트, test 빌드 정리
coast build --type snap  # latest-snap 업데이트, snap 빌드 정리
```

`test` 타입을 빌드해도 `default` 또는 `snap` 빌드에는 영향을 주지 않습니다. 정리는 타입별로 완전히 독립적입니다.

## 타입이 지정된 Coast 실행

`--type`으로 생성된 인스턴스에는 해당 타입 태그가 붙습니다. 같은 프로젝트에 대해 서로 다른 타입의 인스턴스를 동시에 실행할 수 있습니다:

```bash
coast run dev-1                    # 기본 타입
coast run test-1 --type test       # 테스트 타입
coast run snapshot-1 --type snap   # 스냅샷 타입

coast ls
# 세 개 모두 표시되며, 각각 자체 타입, 포트, 볼륨 전략을 가짐
```

이렇게 하면 동일한 프로젝트에 대해 전체 개발 환경을 실행하면서도, 격리된 테스트 러너와 스냅샷으로 시드된 인스턴스를 함께, 동시에 실행할 수 있습니다.
