# Volume Topology

Coast는 데이터 집약적 서비스(데이터베이스, 캐시 등)가 Coast 인스턴스 전반에서 데이터를 저장하고 공유하는 방식을 제어하는 세 가지 볼륨 전략을 제공합니다. 올바른 전략을 선택하는 것은 필요한 격리 수준과 감내할 수 있는 오버헤드의 정도에 달려 있습니다.

## Shared Services

[공유 서비스](SHARED_SERVICES.md)는 어떤 Coast 컨테이너 밖에서, 호스트 Docker 데몬에서 실행됩니다. Postgres, MongoDB, Redis 같은 서비스는 호스트 머신에 그대로 유지되고, Coast 인스턴스는 브리지 네트워크를 통해 호스트로 다시 라우팅하여 호출합니다.

```text
Host machine
  |
  +--> Postgres (host daemon, existing volume)
  +--> Redis (host daemon, existing volume)
  |
  +--> Coast: dev-1  --connects to--> host Postgres, host Redis
  +--> Coast: dev-2  --connects to--> host Postgres, host Redis
```

인스턴스 간 데이터 격리는 없습니다 — 모든 Coast가 동일한 데이터베이스에 연결합니다. 그 대신 다음을 얻을 수 있습니다:

- 자체 데이터베이스 컨테이너를 실행하지 않으므로 Coast 인스턴스가 더 가벼워집니다.
- 기존 호스트 볼륨을 직접 재사용하므로, 이미 가지고 있는 데이터가 즉시 사용 가능합니다.
- 로컬 데이터베이스에 연결하는 MCP 통합이 별도 설정 없이 그대로 동작합니다.

이는 [Coastfile](COASTFILE_TYPES.md)의 `[shared_services]` 아래에서 설정합니다.

## Shared Volumes

공유 볼륨은 모든 Coast 인스턴스에서 공유되는 단일 Docker 볼륨을 마운트합니다. 서비스 자체(Postgres, Redis 등)는 각 Coast 컨테이너 내부에서 실행되지만, 모두 동일한 기본 볼륨에 읽기/쓰기를 수행합니다.

```text
Coast: dev-1  --mounts--> shared volume "my-project-postgres"
Coast: dev-2  --mounts--> shared volume "my-project-postgres"
```

이는 호스트 머신에 있는 것과 Coast 데이터를 분리해 주지만, 인스턴스들끼리는 여전히 데이터를 공유합니다. 호스트 개발 환경과의 깔끔한 분리를 원하면서도 인스턴스별 볼륨의 오버헤드는 피하고 싶을 때 유용합니다.

```toml
[volumes.postgres_data]
strategy = "shared"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

## Isolated Volumes

격리 볼륨은 각 Coast 인스턴스에 독립적인 볼륨을 제공합니다. 인스턴스 간에도, 호스트와도 어떤 데이터도 공유되지 않습니다. 각 인스턴스는 비어 있는 상태로 시작하며(또는 스냅샷에서 시작 — 아래 참조), 독립적으로 분기됩니다.

```text
Coast: dev-1  --mounts--> volume "dev-1-postgres"
Coast: dev-2  --mounts--> volume "dev-2-postgres"
```

이는 병렬 환경 간 진정한 볼륨 격리가 필요하고 통합 테스트 비중이 큰 프로젝트에 가장 좋은 선택입니다. 단점은 각 인스턴스가 자체 데이터 사본을 유지하므로 시작이 더 느리고 Coast 빌드가 더 커진다는 점입니다.

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

## Snapshotting

공유 및 격리 전략은 기본적으로 빈 볼륨으로 시작합니다. 인스턴스가 기존 호스트 볼륨의 복사본으로 시작하게 하려면, `snapshot_source`를 복사할 Docker 볼륨 이름으로 설정하세요:

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

스냅샷은 [빌드 시점](BUILDS.md)에 생성됩니다. 생성 이후에는 각 인스턴스의 볼륨이 독립적으로 분기되며 — 변경 사항은 소스나 다른 인스턴스로 전파되지 않습니다.

Coast는 아직 런타임 스냅샷(예: 실행 중인 인스턴스의 볼륨을 스냅샷으로 생성)을 지원하지 않습니다. 이는 향후 릴리스에서 제공될 예정입니다.
