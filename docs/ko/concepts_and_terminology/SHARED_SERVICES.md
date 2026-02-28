# 공유 서비스

공유 서비스는 Coast 내부가 아니라 호스트 Docker 데몬에서 실행되는 데이터베이스 및 인프라 컨테이너(Postgres, Redis, MongoDB 등)입니다. Coast 인스턴스는 브리지 네트워크를 통해 이들에 연결하므로, 모든 Coast는 동일한 호스트 볼륨에서 동일한 호스트의 동일한 서비스와 통신합니다.

![Shared services in Coastguard](../../assets/coastguard-shared-services.png)
*호스트에서 관리되는 Postgres, Redis, MongoDB를 보여주는 Coastguard 공유 서비스 탭.*

## 작동 방식

Coastfile에 공유 서비스를 선언하면, Coast는 이를 호스트 데몬에서 시작하고 각 Coast 컨테이너 내부에서 실행되는 compose 스택에서는 해당 서비스를 제거합니다. 이후 Coast들은 연결을 호스트로 다시 라우팅하도록 구성됩니다.

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

공유 서비스는 기존 호스트 볼륨을 재사용하므로, 로컬에서 `docker-compose up`을 실행해 이미 가지고 있던 데이터가 즉시 Coast에서 사용 가능합니다.

## 공유 서비스를 사용해야 하는 경우

- 프로젝트에 로컬 데이터베이스에 연결하는 MCP 통합이 있는 경우 — 공유 서비스를 사용하면 재구성 없이 그대로 동작할 수 있습니다. `localhost:5432`에 연결하는 호스트의 데이터베이스 MCP는 공유 Postgres가 동일한 포트로 호스트에서 실행되므로 계속 동작합니다. 동적 포트 탐색도, MCP 재구성도 필요 없습니다. 자세한 내용은 [MCP Servers](MCP_SERVERS.md)를 참고하세요.
- 각 Coast 인스턴스가 자체 데이터베이스 컨테이너를 실행할 필요가 없으므로 Coast 인스턴스를 더 가볍게 유지하고 싶은 경우.
- Coast 인스턴스 간 데이터 격리가 필요 없는 경우(모든 인스턴스가 동일한 데이터를 봅니다).
- 호스트에서 코딩 에이전트를 실행하고(참조: [Filesystem](FILESYSTEM.md)), [`coast exec`](EXEC_AND_DOCKER.md)를 거치지 않고 데이터베이스 상태에 접근시키고 싶은 경우. 공유 서비스를 사용하면 에이전트의 기존 데이터베이스 도구와 MCP가 변경 없이 그대로 동작합니다.

격리가 필요한 경우의 대안은 [Volume Topology](VOLUMES.md) 페이지를 참고하세요.

## 볼륨 식별 주의 경고

Docker 볼륨 이름은 항상 전역적으로 유일하지는 않습니다. 서로 다른 여러 프로젝트에서 `docker-compose up`을 실행한 경우, Coast가 공유 서비스에 연결하는 호스트 볼륨이 예상과 다를 수 있습니다.

공유 서비스를 사용해 Coast를 시작하기 전에, 마지막으로 실행한 `docker-compose up`이 Coast와 함께 사용하려는 프로젝트에서 실행한 것인지 확인하세요. 이렇게 하면 호스트 볼륨이 Coastfile이 기대하는 것과 일치합니다.

## 문제 해결

공유 서비스가 잘못된 호스트 볼륨을 가리키는 것처럼 보인다면:

1. [Coastguard](COASTGUARD.md) UI(`coast ui`)를 엽니다.
2. **Shared Services** 탭으로 이동합니다.
3. 영향을 받는 서비스를 선택하고 **Remove**를 클릭합니다.
4. **Refresh Shared Services**를 클릭하여 현재 Coastfile 구성으로부터 다시 생성합니다.

이 과정은 공유 서비스 컨테이너를 내리고 다시 생성하며, 올바른 호스트 볼륨에 다시 연결합니다.
