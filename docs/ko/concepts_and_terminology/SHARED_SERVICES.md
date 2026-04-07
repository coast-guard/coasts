# 공유 서비스

공유 서비스는 각 Coast 내부가 아니라 호스트 Docker 데몬에서 실행되는 데이터베이스 및 인프라 컨테이너(Postgres, Redis, MongoDB 등)입니다. Coast 인스턴스는 브리지 네트워크를 통해 이들에 연결하므로, 모든 Coast는 동일한 호스트 볼륨에 있는 동일한 서비스와 통신합니다.

![Shared services in Coastguard](../../assets/coastguard-shared-services.png)
*호스트에서 관리되는 Postgres, Redis, MongoDB를 보여주는 Coastguard 공유 서비스 탭.*

## 작동 방식

Coastfile에서 공유 서비스를 선언하면, Coast는 이를 호스트 데몬에서 시작하고 각 Coast 컨테이너 내부에서 실행되는 compose 스택에서는 이를 제거합니다. 그런 다음 Coasts는 서비스 이름 트래픽을 다시 공유 컨테이너로 라우팅하도록 구성되며, 이때 Coast 내부에서는 서비스의 컨테이너 측 포트가 유지됩니다.

```text
Host Docker daemon
  |
  +--> postgres (host volume: infra_postgres_data)
  +--> redis    (host volume: infra_redis_data)
  +--> mongodb  (host volume: infra_mongodb_data)
  |
  +--> Coast: dev-1  --bridge network--> host postgres, redis, mongodb
  +--> Coast: dev-2  --bridge network--> host postgres, redis, mongodb
```

공유 서비스는 기존 호스트 볼륨을 재사용하므로, 로컬에서 `docker-compose up`을 실행하며 이미 보유하고 있던 모든 데이터는 즉시 Coasts에서 사용할 수 있습니다.

매핑된 포트를 사용할 때는 이 구분이 중요합니다:

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- 호스트에서는 공유 서비스가 `localhost:5433`에 게시됩니다.
- 모든 Coast 내부에서는 앱 컨테이너가 계속 `postgis:5432`에 연결합니다.
- `5432`와 같은 정수 하나만 쓰는 방식은 동일 매핑 `"5432:5432"`의 축약형입니다.

## 공유 서비스를 사용해야 하는 경우

- 프로젝트에 로컬 데이터베이스에 연결하는 MCP 통합이 있는 경우 — 공유 서비스를 사용하면 동적 포트 검색 없이도 이것들이 계속 동작합니다. 이미 사용 중인 동일한 호스트 포트로 공유 서비스를 게시하면(예: `ports = [5432]`), 해당 도구는 변경 없이 계속 동작합니다. 다른 호스트 포트로 게시하면(예: `"5433:5432"`), 호스트 측 도구는 그 호스트 포트를 사용해야 하며 Coasts는 계속 컨테이너 포트를 사용합니다.
- Coast 인스턴스가 자체 데이터베이스 컨테이너를 실행할 필요가 없으므로 더 가볍게 유지하고 싶은 경우.
- Coast 인스턴스 간 데이터 격리가 필요하지 않은 경우(모든 인스턴스가 동일한 데이터를 봄).
- 호스트에서 코딩 에이전트를 실행 중이며([Filesystem](FILESYSTEM.md) 참조), [`coast exec`](EXEC_AND_DOCKER.md)를 거치지 않고 데이터베이스 상태에 접근하게 하려는 경우. 공유 서비스를 사용하면 에이전트의 기존 데이터베이스 도구와 MCP가 변경 없이 동작합니다.

격리가 필요한 경우의 대안은 [Volume Topology](VOLUMES.md) 페이지를 참조하세요.

## 볼륨 식별 경고

Docker 볼륨 이름은 항상 전역적으로 고유하지 않습니다. 여러 다른 프로젝트에서 `docker-compose up`을 실행하면, Coast가 공유 서비스에 연결하는 호스트 볼륨이 기대한 것이 아닐 수 있습니다.

공유 서비스와 함께 Coasts를 시작하기 전에, 마지막으로 실행한 `docker-compose up`이 Coasts와 함께 사용하려는 프로젝트에서 실행된 것이 맞는지 확인하세요. 이렇게 하면 호스트 볼륨이 Coastfile의 기대와 일치하게 됩니다.

## 문제 해결

공유 서비스가 잘못된 호스트 볼륨을 가리키는 것처럼 보이는 경우:

1. [Coastguard](COASTGUARD.md) UI를 엽니다(`coast ui`).
2. **Shared Services** 탭으로 이동합니다.
3. 영향을 받는 서비스를 선택하고 **Remove**를 클릭합니다.
4. **Refresh Shared Services**를 클릭하여 현재 Coastfile 구성에서 다시 생성합니다.

이렇게 하면 공유 서비스 컨테이너가 제거되고 다시 생성되며, 올바른 호스트 볼륨에 다시 연결됩니다.

## 공유 서비스와 원격 Coast

[원격 coasts](REMOTES.md)를 실행할 때에도 공유 서비스는 계속 로컬 머신에서 실행됩니다. 데몬은 SSH 리버스 터널(`ssh -R`)을 설정하여 원격 DinD 컨테이너가 `host.docker.internal`을 통해 이들에 도달할 수 있게 합니다. 이를 통해 로컬 데이터베이스가 원격 인스턴스와도 공유됩니다. 리버스 터널이 올바르게 바인딩되려면 원격 호스트의 sshd에서 `GatewayPorts clientspecified`가 활성화되어 있어야 합니다.
