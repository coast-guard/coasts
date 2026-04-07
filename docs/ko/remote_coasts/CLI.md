# CLI 및 구성

이 페이지에서는 `coast remote` 명령 그룹, `Coastfile.remote` 구성 형식, 그리고 원격 머신의 디스크 관리를 다룹니다.

## 원격 관리 명령

### `coast remote add`

데몬에 원격 머신을 등록합니다:

```bash
coast remote add <name> <user>@<host> [--key <path>]
coast remote add <name> <user>@<host>:<port> [--key <path>]
```

예시:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
coast remote add dev-box ec2-user@10.50.56.218:22 --key ~/.ssh/coast_key
```

연결 세부 정보는 데몬의 `state.db`에 저장됩니다. 이 정보는 Coastfile에는 절대 저장되지 않습니다.

### `coast remote ls`

등록된 모든 원격을 나열합니다:

```bash
coast remote ls
```

### `coast remote rm`

등록된 원격을 제거합니다:

```bash
coast remote rm <name>
```

원격에서 인스턴스가 아직 실행 중이면, 먼저 `coast rm`으로 제거하세요.

### `coast remote test`

SSH 연결성과 coast-service 사용 가능 여부를 확인합니다:

```bash
coast remote test <name>
```

이 명령은 SSH 접근을 확인하고, SSH 터널을 통해 포트 31420에서 coast-service에 도달 가능한지 확인하며, 원격의 아키텍처와 coast-service 버전을 보고합니다.

### `coast remote prune`

원격 머신의 고아 리소스를 정리합니다:

```bash
coast remote prune <name>              # 고아 리소스 제거
coast remote prune <name> --dry-run    # 제거될 항목 미리 보기
```

Prune은 Docker 볼륨과 워크스페이스 디렉터리를 coast-service 인스턴스 데이터베이스와 교차 참조하여 고아 리소스를 식별합니다. 활성 인스턴스에 속한 리소스는 절대 제거되지 않습니다.

## Coastfile 구성

원격 coast는 기본 구성을 확장하는 별도의 Coastfile을 사용합니다. 파일 이름이 유형을 결정합니다:

| File | Type |
|------|------|
| `Coastfile.remote` | `remote` |
| `Coastfile.remote.toml` | `remote` |
| `Coastfile.remote.light` | `remote.light` |
| `Coastfile.remote.light.toml` | `remote.light` |

### 최소 예제

```toml
[coast]
name = "my-app"
extends = "Coastfile"

[remote]
workspace_sync = "mutagen"
```

### `[remote]` 섹션

`[remote]` 섹션은 동기화 기본 설정을 선언합니다. 연결 세부 정보(호스트, 사용자, SSH 키)는 `coast remote add`에서 가져오며 런타임에 확인됩니다.

| Field | Default | Description |
|-------|---------|-------------|
| `workspace_sync` | `"rsync"` | 동기화 전략: `"rsync"`는 일회성 대량 전송만, `"mutagen"`은 rsync + 지속적인 실시간 동기화 |

### 검증 제약 조건

1. Coastfile 유형이 `remote`로 시작하는 경우 `[remote]` 섹션이 필요합니다.
2. 원격이 아닌 Coastfile에는 `[remote]` 섹션이 있을 수 없습니다.
3. 인라인 호스트 구성은 지원되지 않습니다. 연결 세부 정보는 등록된 원격에서 가져와야 합니다.
4. `strategy = "shared"`인 공유 볼륨은 원격 호스트에 Docker 볼륨을 생성하며, 해당 원격의 모든 coast 간에 공유됩니다. 이 볼륨은 서로 다른 원격 머신 간에는 분산되지 않습니다.

### 상속

원격 Coastfile은 다른 유형의 Coastfile과 동일한 [상속 시스템](../coastfiles/INHERITANCE.md)을 사용합니다. `extends = "Coastfile"` 지시문은 기본 구성과 원격 재정의를 병합합니다. 다른 유형 변형과 마찬가지로 포트, 서비스, 볼륨을 재정의하고 전략을 지정할 수 있습니다.

## 디스크 관리

### 인스턴스별 리소스 사용량

각 원격 coast 인스턴스는 대략 다음을 소비합니다:

| Resource | Size | Location |
|----------|------|----------|
| DinD Docker volume | 3-5 GB | Remote Docker storage |
| Workspace directory | 50-300 MB | `/data/workspaces/{project}/{instance}` |
| Image tarballs | 2-3 GB | `/data/image-cache/*.tar` (shared across instances) |
| Build artifacts | 200-500 MB | `/data/images/{project}/{build_id}/` |

권장 최소 디스크 용량: 일반적인 프로젝트에서 동시 인스턴스 2~3개 기준 **50 GB**.

### 리소스 명명 규칙

| Resource | Naming pattern |
|----------|---------------|
| DinD volume | `coast-dind--{project}--{instance}` |
| Workspace | `/data/workspaces/{project}/{instance}` |
| Image cache | `/data/image-cache/*.tar` |
| Build artifacts | `/data/images/{project}/{build_id}/` |

### `coast rm` 시 정리

`coast rm`이 원격 인스턴스를 제거할 때 다음을 정리합니다:

1. 원격 DinD 컨테이너(coast-service를 통해)
2. DinD Docker 볼륨 (`coast-dind--{project}--{name}`)
3. 워크스페이스 디렉터리 (`/data/workspaces/{project}/{name}`)
4. 로컬 섀도 인스턴스 레코드, 포트 할당, 그리고 셸 컨테이너

### prune을 수행해야 하는 시점

인스턴스를 제거한 후 원격에서 `df -h`가 높은 디스크 사용량을 보여준다면, 실패했거나 중단된 작업 때문에 고아 리소스가 남아 있을 수 있습니다. 공간을 회수하려면 `coast remote prune`을 실행하세요:

```bash
# 제거될 항목 보기
coast remote prune my-vm --dry-run

# 실제로 제거
coast remote prune my-vm
```

Prune은 활성 인스턴스에 속한 리소스를 절대 제거하지 않습니다.
