# 런타임과 서비스

Coast는 컨테이너 런타임 안에서 실행됩니다 — 자체 Docker(또는 Podman) 데몬을 호스팅하는 바깥쪽 컨테이너입니다. 프로젝트의 서비스는 그 내부 데몬 안에서 실행되며, 다른 Coast 인스턴스와 완전히 격리됩니다. 현재 **프로덕션에서 검증된 런타임은 DinD(Docker-in-Docker)뿐입니다.** 지금은 Podman과 Sysbox 지원이 충분히 테스트될 때까지 DinD를 계속 사용하시길 권장합니다.

## 런타임

Coastfile의 `runtime` 필드는 Coast를 구동하는 컨테이너 런타임을 선택합니다. 기본값은 `dind`이며, 아예 생략할 수도 있습니다:

```toml
[coast]
name = "my-app"
runtime = "dind"
```

허용되는 값은 `dind`, `sysbox`, `podman` 세 가지입니다. 실제로는 DinD만 데몬에 연결되어 있으며 엔드투엔드로 테스트되었습니다.

### DinD (Docker-in-Docker)

기본값이며, 현재로서는 여러분이 사용해야 하는 유일한 런타임입니다. Coast는 `docker:dind` 이미지로부터 `--privileged` 모드를 활성화해 컨테이너를 생성합니다. 그 컨테이너 내부에서 완전한 Docker 데몬이 기동되고, 여러분의 `docker-compose.yml` 서비스가 중첩 컨테이너로 실행됩니다.

DinD는 완전히 통합되어 있습니다:

- 이미지는 호스트에 사전 캐시되며 `coast run` 시 내부 데몬으로 로드됩니다
- 인스턴스별 이미지는 호스트에서 빌드된 뒤 `docker save | docker load`로 파이프되어 들어갑니다
- 내부 데몬의 상태는 `/var/lib/docker`에 있는 이름 있는 볼륨(`coast-dind--{project}--{instance}`)에 영속화되므로, 이후 실행에서는 이미지 로딩을 완전히 건너뜁니다
- 포트는 DinD 컨테이너에서 호스트로 직접 퍼블리시됩니다
- Compose 오버라이드, 공유 서비스 네트워크 브리징, 시크릿 주입, 볼륨 전략이 모두 동작합니다

### Sysbox (future)

Sysbox는 `--privileged` 없이 루트리스 컨테이너를 제공하는 Linux 전용 OCI 런타임입니다. 이는 privileged 모드 대신 `--runtime=sysbox-runc`를 사용하게 되며, 더 나은 보안 자세입니다. 트레이트 구현은 코드베이스에 존재하지만 데몬에 연결되어 있지 않습니다. macOS에서는 동작하지 않습니다.

### Podman (future)

Podman은 내부 Docker 데몬을 `quay.io/podman/stable` 안에서 실행되는 Podman 데몬으로 대체하며, `docker compose` 대신 `podman-compose`를 사용합니다. 트레이트 구현은 존재하지만 데몬에 연결되어 있지 않습니다.

Sysbox와 Podman 지원이 안정화되면 이 페이지가 업데이트될 것입니다. 지금은 `runtime`을 `dind`로 두거나 생략하세요.

## Docker-in-Docker 아키텍처

모든 Coast는 중첩 컨테이너입니다. 호스트 Docker 데몬이 바깥쪽 DinD 컨테이너를 관리하고, 그 안의 내부 Docker 데몬이 여러분의 compose 서비스를 관리합니다.

```text
Host machine
│
├── Docker daemon (host)
│   │
│   ├── coast container: dev-1 (docker:dind, --privileged)
│   │   │
│   │   ├── Inner Docker daemon
│   │   │   ├── web        (your app, :3000)
│   │   │   ├── postgres   (database, :5432)
│   │   │   └── redis      (cache, :6379)
│   │   │
│   │   ├── /workspace          ← bind mount of your project root
│   │   ├── /image-cache        ← read-only mount of ~/.coast/image-cache/
│   │   ├── /coast-artifact     ← read-only mount of the build artifact
│   │   ├── /coast-override     ← generated compose overrides
│   │   └── /var/lib/docker     ← named volume (inner daemon state)
│   │
│   ├── coast container: dev-2 (docker:dind, --privileged)
│   │   └── (same structure, fully isolated)
│   │
│   └── shared postgres (host-level, bridge network)
│
└── ~/.coast/
    ├── image-cache/    ← OCI tarballs shared across all projects
    └── state.db        ← instance metadata
```

`coast run`이 인스턴스를 생성할 때, 다음을 수행합니다:

1. 호스트 데몬에서 DinD 컨테이너를 생성하고 시작합니다
2. 내부 데몬이 준비될 때까지(최대 120초) 컨테이너 안에서 `docker info`를 폴링합니다
3. 내부 데몬이 이미 가지고 있는 이미지(영속 `/var/lib/docker` 볼륨에서)를 확인하고, 캐시에서 누락된 tarball을 로드합니다
4. 호스트에서 빌드된 인스턴스별 이미지를 `docker save | docker load`로 파이프해 넣습니다
5. `/host-project`를 `/workspace`에 바인드하여 compose 서비스가 소스 코드를 볼 수 있게 합니다
6. 컨테이너 안에서 `docker compose up -d`를 실행하고 모든 서비스가 실행 중이거나 healthy가 될 때까지 기다립니다

영속 `/var/lib/docker` 볼륨이 핵심 최적화입니다. 처음 `coast run`에서는 내부 데몬으로 이미지를 로드하는 데 20초 이상 걸릴 수 있습니다. 이후 실행에서는(`coast rm` 후 재실행하더라도) 내부 데몬이 이미지를 이미 캐시하고 있어 시작 시간이 10초 미만으로 줄어듭니다.

## 서비스

서비스는 Coast 내부에서 실행되는 컨테이너(또는 [bare services](BARE_SERVICES.md)의 경우 프로세스)입니다. compose 기반 Coast의 경우, 이는 `docker-compose.yml`에 정의된 서비스들입니다.

![Services tab in Coastguard](../../assets/coastguard-services.png)
*compose 서비스, 상태, 이미지, 포트 매핑을 보여주는 Coastguard Services 탭.*

Coastguard의 Services 탭은 Coast 인스턴스 내부에서 실행 중인 모든 서비스를 보여줍니다:

- **Service** — compose 서비스 이름(예: `web`, `backend`, `redis`). 클릭하면 해당 컨테이너의 상세 inspect 데이터, 로그, 통계를 볼 수 있습니다.
- **Status** — 서비스가 실행 중인지, 중지되었는지, 오류 상태인지 여부입니다.
- **Image** — 서비스가 빌드되는 기반 Docker 이미지입니다.
- **Ports** — 원시 compose 포트 매핑과 coast가 관리하는 [canonical/dynamic ports](PORTS.md)입니다. 동적 포트는 항상 접근 가능하며, canonical 포트는 [체크아웃된](CHECKOUT.md) 인스턴스로만 라우팅됩니다.

여러 서비스를 선택한 뒤 툴바에서 일괄 중지, 시작, 재시작, 또는 제거할 수 있습니다.

[shared services](SHARED_SERVICES.md)로 구성된 서비스는 Coast 내부가 아니라 호스트 데몬에서 실행되므로 이 목록에 나타나지 않습니다. 이들은 별도의 탭을 가집니다.

## `coast ps`

Services 탭의 CLI 대응 기능은 `coast ps`입니다:

```bash
coast ps dev-1
```

```text
Services in coast instance 'dev-1':
  NAME                      STATUS               PORTS
  backend                   running              0.0.0.0:8080->8080/tcp, 0.0.0.0:40000->40000/tcp
  mailhog                   running              0.0.0.0:1025->1025/tcp, 0.0.0.0:8025->8025/tcp
  reach-web                 running              0.0.0.0:4000->4000/tcp
  test-redis                running              0.0.0.0:6380->6379/tcp
  web                       running              0.0.0.0:3000->3000/tcp
```

내부적으로 데몬은 DinD 컨테이너 안에서 `docker compose ps --format json`를 실행하고 JSON 출력을 파싱합니다. 결과는 반환되기 전에 여러 필터를 거칩니다:

- **공유 서비스(Shared services)** 는 제거됩니다 — 이들은 Coast 내부가 아니라 호스트에서 실행됩니다.
- **원샷 작업(One-shot jobs)** (포트가 없는 서비스)은 성공적으로 종료되면 숨겨집니다. 실패하면 조사할 수 있도록 표시됩니다.
- **누락된 서비스(Missing services)** — 존재해야 하는 장시간 실행 서비스가 출력에 없다면, 문제가 있음을 알 수 있도록 `down` 상태로 추가됩니다.

더 깊게 점검하려면 `coast logs`로 서비스 출력을 tail하고, [`coast exec`](EXEC_AND_DOCKER.md)으로 Coast 컨테이너 안에서 셸을 띄우세요. 로그 스트리밍과 MCP 트레이드오프에 대한 전체 내용은 [Logs](LOGS.md)를 참고하세요.

```bash
coast logs dev-1 --service web --tail 100
coast exec dev-1
```
