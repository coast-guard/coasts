# 아키텍처

원격 coast는 실행을 로컬 머신과 원격 서버 사이에 분리합니다. 데몬이 모든 작업을 SSH 터널을 통해 투명하게 라우팅하므로 개발자 경험은 바뀌지 않습니다.

## 두 컨테이너 분할

모든 원격 coast는 두 개의 컨테이너를 생성합니다:

### Shell Coast (로컬)

사용자 머신에서 실행되는 경량 Docker 컨테이너입니다. 일반 coast와 동일한 bind mount(`/host-project`, `/workspace`)를 가지지만 내부 Docker 데몬과 compose 서비스는 없습니다. 엔트리포인트는 `sleep infinity`입니다.

shell coast는 한 가지 이유로 존재합니다: 호스트 측 에이전트와 에디터가 `/workspace` 아래의 파일을 수정할 수 있도록 [filesystem bridge](../concepts_and_terminology/FILESYSTEM.md)를 유지합니다. 이러한 수정 사항은 [rsync and mutagen](FILE_SYNC.md)을 통해 원격으로 동기화됩니다.

### Remote Coast (원격)

원격 머신에서 `coast-service`가 관리합니다. 실제 작업이 이루어지는 곳입니다: compose 서비스를 실행하는 전체 DinD 컨테이너이며, 각 서비스에 대해 동적 포트가 할당됩니다.

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ LOCAL MACHINE                                                            │
│                                                                          │
│  ┌────────────┐    unix     ┌───────────────────────────────────────┐    │
│  │ coast CLI  │───socket───▶│ coast-daemon                         │    │
│  └────────────┘             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Shell Coast (sleep infinity)    │  │    │
│                             │  │ - /host-project (bind mount)    │  │    │
│                             │  │ - /workspace (mount --bind)     │  │    │
│                             │  │ - NO inner docker               │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Port Manager                    │  │    │
│                             │  │ - allocates local dynamic ports │  │    │
│                             │  │ - SSH -L tunnels to remote      │  │    │
│                             │  │   dynamic ports                 │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Shared Services (local)         │  │    │
│                             │  │ - postgres, redis, etc.         │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  state.db (shadow instance,           │    │
│                             │           remote_host, port allocs)   │    │
│                             └───────────────────┬───────────────────┘    │
│                                                 │                        │
│                                    SSH tunnel   │  rsync / SSH           │
│                                                 │                        │
└─────────────────────────────────────────────────┼────────────────────────┘
                                                  │
┌─────────────────────────────────────────────────┼────────────────────────┐
│ REMOTE MACHINE                                  │                        │
│                                                 ▼                        │
│  ┌───────────────────────────────────────────────────────────────────┐   │
│  │ coast-service (HTTP API on :31420)                                │   │
│  │                                                                   │   │
│  │  ┌───────────────────────────────────────────────────────────┐    │   │
│  │  │ DinD Container (per instance)                             │    │   │
│  │  │  /workspace (synced from local)                           │    │   │
│  │  │  compose services / bare services                         │    │   │
│  │  │  published on dynamic ports (e.g. :52340 -> :3000)        │    │   │
│  │  └───────────────────────────────────────────────────────────┘    │   │
│  │                                                                   │   │
│  │  Port Manager (dynamic port allocation per instance)              │   │
│  │  Build artifacts (/data/images/)                                  │   │
│  │  Image cache (/data/image-cache/)                                 │   │
│  │  Keystore (encrypted secrets)                                     │   │
│  │  remote-state.db (instances, worktrees)                           │   │
│  └───────────────────────────────────────────────────────────────────┘   │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

## SSH 터널 계층

데몬은 두 종류의 SSH 터널을 사용해 로컬과 원격을 연결합니다:

### Forward Tunnels (로컬에서 원격으로)

각 서비스 포트에 대해 데몬은 로컬 동적 포트를 해당 원격 동적 포트에 매핑하는 `ssh -L` 터널을 생성합니다. 이것이 `localhost:{dynamic_port}`가 원격 서비스에 도달할 수 있게 하는 방식입니다.

```text
ssh -N -L {local_dynamic}:localhost:{remote_dynamic} user@remote
```

`coast ports`를 실행하면 dynamic 열에 이러한 로컬 터널 엔드포인트가 표시됩니다.

### Reverse Tunnels (원격에서 로컬로)

[Shared services](../concepts_and_terminology/SHARED_SERVICES.md) (Postgres, Redis 등)는 로컬 머신에서 실행됩니다. 데몬은 원격 DinD 컨테이너가 이들에 접근할 수 있도록 `ssh -R` 터널을 생성합니다:

```text
ssh -N -R 0.0.0.0:{remote_port}:localhost:{local_port} user@remote
```

원격 DinD 컨테이너 내부에서 서비스는 `host.docker.internal:{port}`를 통해 shared services에 연결하며, 이는 reverse tunnel이 리스닝 중인 Docker 브리지 게이트웨이로 해석됩니다.

reverse tunnel이 `127.0.0.1` 대신 `0.0.0.0`에 바인딩되도록 하려면 원격 호스트의 sshd에서 `GatewayPorts clientspecified`가 활성화되어 있어야 합니다.

### 터널 복구

노트북이 절전 모드로 들어가거나 네트워크가 변경되면 SSH 터널이 끊어질 수 있습니다. 데몬은 백그라운드 상태 검사 루프를 실행하며 다음을 수행합니다:

1. 5초마다 TCP 연결을 통해 각 동적 포트를 검사합니다.
2. 인스턴스의 모든 포트가 죽어 있으면 해당 인스턴스의 오래된 터널 프로세스를 종료하고 다시 설정합니다.
3. 일부 포트만 죽어 있으면(부분 장애) 정상 포트에는 영향을 주지 않고 누락된 터널만 다시 설정합니다.
4. 새 reverse tunnel을 만들기 전에 `fuser -k`를 통해 오래된 원격 포트 바인딩을 정리합니다.

복구는 인스턴스별로 이루어집니다 -- 한 인스턴스의 터널을 복구해도 다른 인스턴스에는 영향을 주지 않습니다.

## 포트 포워딩 체인

중간 계층의 모든 포트는 동적입니다. 정식 포트는 엔드포인트에만 존재합니다: 서비스가 리스닝하는 DinD 컨테이너 내부와 [`coast checkout`](../concepts_and_terminology/CHECKOUT.md)을 통한 localhost입니다.

```text
localhost:3000 (canonical, via coast checkout / socat)
       ↓
localhost:{local_dynamic} (allocated by daemon port manager)
       ↓ SSH -L tunnel
remote:{remote_dynamic} (allocated by coast-service port manager)
       ↓ Docker port publish
DinD container :3000 (canonical, where the app listens)
```

이 3단계 체인은 하나의 원격 머신에서 동일한 프로젝트의 여러 인스턴스를 포트 충돌 없이 허용합니다. 각 인스턴스는 양쪽에 자체적인 동적 포트 집합을 갖습니다.

## 요청 라우팅

모든 데몬 핸들러는 인스턴스의 `remote_host`를 확인합니다. 설정되어 있으면 요청은 SSH 터널을 통해 coast-service로 전달됩니다:

| Command | Remote behavior |
|---------|-----------------|
| `coast run` | 로컬에서 shell coast 생성 + 아티팩트 전송 + coast-service로 전달 |
| `coast build` | 원격 머신에서 빌드 (로컬 빌드는 전달하지 않음) |
| `coast assign` | 새 worktree 내용 rsync + assign 요청 전달 |
| `coast exec` | coast-service로 전달 |
| `coast ps` | coast-service로 전달 |
| `coast logs` | coast-service로 전달 |
| `coast stop` | 전달 + 로컬 SSH 터널 종료 |
| `coast start` | 전달 + SSH 터널 재설정 |
| `coast rm` | 전달 + 터널 종료 + 로컬 shadow instance 삭제 |
| `coast checkout` | 로컬 전용 (호스트에서 socat, 전달 불필요) |
| `coast secret set` | 로컬에 저장 + 원격 keystore로 전달 |

## coast-service

`coast-service`는 원격 머신에서 실행되는 제어 플레인입니다. 포트 31420에서 리스닝하는 HTTP 서버(Axum)이며, 빌드, 실행, assign, exec, ps, logs, stop, start, rm, secrets, 서비스 재시작 등 데몬의 로컬 작업을 그대로 반영합니다.

자체 SQLite 상태 데이터베이스, Docker 컨테이너(DinD), 동적 포트 할당, 빌드 아티팩트, 이미지 캐시, 암호화된 keystore를 관리합니다. 데몬은 오직 SSH 터널을 통해서만 이와 통신합니다 -- coast-service는 절대 공용 인터넷에 노출되지 않습니다.

배포 지침은 [Setup](SETUP.md)을 참고하세요.
