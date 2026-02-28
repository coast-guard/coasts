# 볼륨

`[volumes.*]` 섹션은 명명된 Docker 볼륨이 Coast 인스턴스 전반에서 어떻게 처리되는지 제어합니다. 각 볼륨은 인스턴스가 데이터를 공유할지, 아니면 각자 독립적인 복사본을 가질지를 결정하는 전략으로 구성됩니다.

공유 서비스라는 대안을 포함해 Coast에서의 데이터 격리에 대한 더 큰 그림은 [Volumes](../concepts_and_terminology/VOLUMES.md)를 참고하세요.

## 볼륨 정의하기

각 볼륨은 `[volumes]` 아래의 이름이 있는 TOML 섹션입니다. 세 가지 필드가 필요합니다:

- **`strategy`** — `"isolated"` 또는 `"shared"`
- **`service`** — 이 볼륨을 사용하는 compose 서비스 이름
- **`mount`** — 볼륨의 컨테이너 마운트 경로

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"
```

## 전략

### `isolated`

각 Coast 인스턴스는 자체적으로 독립된 볼륨을 가집니다. 데이터는 인스턴스 간에 공유되지 않습니다. 볼륨은 `coast run` 시 생성되고 `coast rm` 시 삭제됩니다.

```toml
[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"
```

대부분의 데이터베이스 볼륨에는 이것이 올바른 선택입니다 — 각 인스턴스는 깨끗한 초기 상태에서 시작하며 다른 인스턴스에 영향을 주지 않고 자유롭게 데이터를 변경할 수 있습니다.

### `shared`

모든 Coast 인스턴스가 단일 Docker 볼륨을 사용합니다. 한 인스턴스가 기록한 모든 데이터는 다른 모든 인스턴스에서 볼 수 있습니다.

```toml
[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

공유 볼륨은 `coast rm`에 의해 절대 삭제되지 않습니다. 수동으로 제거할 때까지 유지됩니다.

Coast는 데이터베이스 같은 서비스에 연결된 볼륨에서 `shared`를 사용하면 빌드 시 경고를 출력합니다. 하나의 데이터베이스 볼륨을 여러 동시 인스턴스가 공유하면 손상이 발생할 수 있습니다. 공유 데이터베이스가 필요하다면 대신 [shared services](SHARED_SERVICES.md)를 사용하세요.

공유 볼륨의 좋은 사용처: 의존성 캐시(Go modules, npm cache, pip cache), 빌드 산출물 캐시, 그리고 동시 쓰기가 안전하거나 발생 가능성이 낮은 기타 데이터.

## 스냅샷 시딩

격리된 볼륨은 `snapshot_source`를 사용해 인스턴스 생성 시 기존 Docker 볼륨에서 시딩될 수 있습니다. 소스 볼륨의 데이터가 새로운 격리 볼륨으로 복사되며, 이후에는 독립적으로 분기됩니다.

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "db"
mount = "/var/lib/postgresql/data"
```

`snapshot_source`는 `strategy = "isolated"`에서만 유효합니다. 공유 볼륨에 설정하면 오류입니다.

이는 각 Coast 인스턴스가 호스트 개발 데이터베이스에서 복사한 현실적인 데이터셋으로 시작하길 원하지만, 인스턴스들이 소스나 서로에게 영향을 주지 않고 해당 데이터를 자유롭게 변경할 수 있어야 할 때 유용합니다.

## 예시

### 격리된 데이터베이스, 공유 의존성 캐시

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"

[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

### 스냅샷으로 시딩된 풀 스택

각 인스턴스는 호스트에 존재하는 데이터베이스 볼륨의 복사본으로 시작한 뒤, 독립적으로 분기합니다.

```toml
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

### 인스턴스별로 깨끗한 데이터베이스를 사용하는 테스트 러너

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
service = "mongodb"
mount = "/data/db"
```
