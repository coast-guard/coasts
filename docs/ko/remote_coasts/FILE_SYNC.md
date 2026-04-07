# 파일 동기화

원격 coast는 2계층 동기화 전략을 사용합니다: 대량 전송에는 rsync, 지속적인 실시간 동기화에는 mutagen을 사용합니다. 두 도구 모두 coast 컨테이너 내부에 설치되는 런타임 의존성이며, 호스트 머신에는 필요하지 않습니다.

## 동기화가 실행되는 위치

```text
Local Machine                          Remote Machine
┌─────────────────────────────┐        ┌──────────────────────────────┐
│  coastd daemon              │        │                              │
│    │                        │        │                              │
│    │ rsync (direct SSH)     │  SSH   │  /data/workspaces/{p}/{i}/   │
│    │────────────────────────│───────▶│    (rsync writes here)       │
│    │                        │        │    │                         │
│    │ docker exec            │        │    │ bind mount              │
│    ▼                        │        │    ▼                         │
│  Shell Container            │  SSH   │  Remote DinD Container       │
│    /workspace (bind mount)  │───────▶│    /workspace                │
│    mutagen (continuous sync)│        │    (compose services running)│
│    SSH key (copied in)      │        │                              │
└─────────────────────────────┘        └──────────────────────────────┘
```

데몬은 호스트 프로세스에서 직접 rsync를 실행합니다. Mutagen은 로컬 셸 컨테이너 내부에서 `docker exec`를 통해 실행됩니다.

## 계층 1: rsync (대량 전송)

`coast run` 및 `coast assign` 시, 데몬은 호스트에서 rsync를 실행하여 워크스페이스 파일을 원격으로 전송합니다:

```bash
rsync -rlDzP --delete-after \
  --rsync-path="sudo rsync" \
  --exclude '.git' --exclude 'node_modules' \
  --exclude 'target' --exclude '__pycache__' \
  --exclude '.react-router' --exclude '.next' \
  -e "ssh -p {port} -i {key}" \
  {local_workspace}/ {user}@{host}:{remote_workspace}/
```

rsync가 완료된 후, 데몬은 원격에서 `sudo chown -R`을 실행하여 SSH 사용자에게 파일 소유권을 부여합니다. rsync는 `--rsync-path="sudo rsync"`를 통해 root로 실행되는데, 이는 원격 워크스페이스에 컨테이너 내부의 coast-service 작업으로 인해 root 소유 파일이 포함될 수 있기 때문입니다.

### rsync가 잘하는 것

- **초기 전송.** 첫 번째 `coast run`은 전체 워크스페이스를 전송합니다.
- **워크트리 전환.** `coast assign`은 이전 워크트리와 새 워크트리 간의 델타만 전송합니다. 변경되지 않은 파일은 다시 전송되지 않습니다.
- **압축.** `-z` 플래그는 전송 중 데이터를 압축합니다.

### 제외되는 경로

rsync는 전송해서는 안 되는 경로를 건너뜁니다:

| Path | Why |
|------|-----|
| `.git` | 크고, 원격에서는 필요하지 않음 (워크트리 내용만으로 충분함) |
| `node_modules` | lockfile로부터 DinD 내부에서 다시 빌드됨 |
| `target` | Rust/Go 빌드 아티팩트로, 원격에서 다시 빌드됨 |
| `__pycache__` | Python 바이트코드 캐시로, 다시 생성됨 |
| `.react-router` | 생성된 타입으로, dev 서버가 다시 생성함 |
| `.next` | Next.js 빌드 캐시로, 다시 생성됨 |

### 생성된 파일 보호

`coast assign`이 `--delete-after`와 함께 실행되면, rsync는 일반적으로 로컬에 존재하지 않는 파일을 원격에서 삭제합니다. 이렇게 되면 원격 dev 서버가 생성했지만 로컬 워크트리에는 없는 생성 파일(예: `generated/`의 proto 클라이언트)이 삭제됩니다.

이를 방지하기 위해 rsync는 특정 생성 디렉터리를 삭제로부터 보호하는 `--filter 'P generated/***'` 규칙을 사용합니다. 보호되는 경로에는 `generated/`, `.react-router/`, `internal/generated/`, `app/generated/`가 포함됩니다.

### 부분 전송 처리

rsync 종료 코드 23(부분 전송)은 치명적이지 않은 경고로 처리됩니다. 이는 원격 DinD 내부에서 실행 중인 dev 서버가 rsync가 쓰는 동안 파일(예: `.react-router/types/`)을 다시 생성하는 경쟁 조건을 처리하기 위함입니다. 소스 파일은 성공적으로 전송되며, 실패할 수 있는 것은 생성 아티팩트뿐이고, 어차피 dev 서버가 다시 생성합니다.

## 계층 2: mutagen (지속적 동기화)

초기 rsync 후, 데몬은 로컬 셸 컨테이너 내부에서 mutagen 세션을 시작합니다:

```bash
docker exec {shell_container} mutagen sync create \
    --name coast-{project}-{instance} \
    --sync-mode one-way-safe \
    --ignore-vcs \
    --ignore node_modules --ignore target \
    --ignore __pycache__ --ignore .next \
    /workspace/ {user}@{host}:{remote_workspace}/
```

Mutagen은 OS 수준 이벤트(컨테이너 내부의 inotify)를 통해 파일 변경을 감시하고, 변경 사항을 배치 처리한 뒤, 지속적인 SSH 연결을 통해 델타를 전송합니다. 여러분의 편집 내용은 몇 초 내에 원격에 반영됩니다.

### One-way-safe 모드

Mutagen은 `one-way-safe` 모드로 실행됩니다: 변경 사항은 로컬에서 원격으로만 흐릅니다. 원격에서 생성된 파일(dev 서버, 빌드 도구 등으로 생성됨)은 로컬 머신으로 다시 동기화되지 않습니다. 이렇게 하면 생성된 아티팩트가 작업 디렉터리를 오염시키는 것을 방지할 수 있습니다.

### Mutagen은 런타임 의존성입니다

Mutagen은 다음 위치에 설치됩니다:

- **coast image** (`[coast.setup]`에서 `coast build`로 빌드됨): 로컬 셸 컨테이너에서 사용됩니다.
- **coast-service Docker image** (`Dockerfile.coast-service`): 원격 측에서 사용됩니다.

데몬은 호스트에서 직접 mutagen을 실행하지 않습니다. 대신 셸 컨테이너에 `docker exec`로 들어가 오케스트레이션합니다.

## 수명 주기

| Command | rsync | mutagen |
|---------|-------|---------|
| `coast run` | 초기 전체 전송 | rsync 후 세션 생성 |
| `coast assign` | 새 워크트리의 델타 전송 | 이전 세션 종료, 새 세션 생성 |
| `coast stop` | -- | 세션 종료 |
| `coast rm` | -- | 세션 종료 |

### 폴백 동작

셸 컨테이너 내부에서 mutagen 세션 시작에 실패하면, 데몬은 경고를 기록합니다. 초기 rsync는 여전히 워크스페이스 내용을 제공합니다. 하지만 세션이 다시 수립될 때까지(예: 다음 `coast assign` 또는 데몬 재시작 시) 파일 변경 사항은 실시간으로 동기화되지 않습니다.

## 동기화 전략 구성

Coastfile의 `[remote]` 섹션은 동기화 전략을 제어합니다:

```toml
[remote]
workspace_sync = "mutagen"    # "rsync" (default) or "mutagen"
```

- **`rsync`** (기본값): 초기 rsync 전송만 실행됩니다. 지속적 동기화는 없습니다. 실시간 동기화가 필요하지 않은 CI 환경이나 배치 작업에 적합합니다.
- **`mutagen`**: 초기 전송에는 rsync를 사용하고, 이후 지속적 동기화에는 mutagen을 사용합니다. 편집 내용이 즉시 원격에 반영되기를 원하는 대화형 개발에 사용하세요.
