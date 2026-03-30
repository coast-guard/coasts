# Private Paths

여러 Coast 인스턴스가 동일한 프로젝트 루트를 공유하면, 동일한 파일과 동일한 inode도 함께 공유하게 됩니다. 이는 보통 의도된 동작입니다. 호스트에서의 파일 변경 사항이 Coast 내부에 즉시 반영되는 이유는 양쪽이 같은 파일시스템을 보기 때문입니다. 하지만 일부 도구는 워크스페이스에 프로세스별 상태를 기록하며, 이 상태는 배타적 접근을 전제로 합니다. 두 인스턴스가 같은 마운트를 공유하면 그 전제가 깨집니다.

## The Problem

Next.js 16을 예로 들어보겠습니다. 개발 서버가 시작될 때 `.next/dev/lock`에 대해 `flock(fd, LOCK_EX)`를 사용하여 배타적 락을 획득합니다. `flock`은 inode 수준의 커널 메커니즘으로, 마운트 네임스페이스, 컨테이너 경계, 바인드 마운트 경로를 신경 쓰지 않습니다. 두 개의 서로 다른 Coast 컨테이너 안의 두 프로세스가 모두 같은 `.next/dev/lock` inode를 가리키고 있다면(같은 호스트 바인드 마운트를 공유하기 때문), 두 번째 프로세스는 첫 번째 프로세스의 락을 감지하고 시작을 거부합니다.

```text
⨯ Another next dev server is already running.

- Local: http://localhost:3000
- PID: 1361
- Dir: /workspace/frontend
```

동일한 유형의 충돌은 다음에도 적용됩니다:

- `flock` / `fcntl` 권고 락(Next.js, Turbopack, Cargo, Gradle)
- PID 파일(많은 데몬이 PID 파일을 기록하고 시작 시 이를 확인함)
- 단일 작성자 접근을 가정하는 빌드 캐시(Webpack, Vite, esbuild)

마운트 네임스페이스 격리(`unshare`)는 여기서 도움이 되지 않습니다. 마운트 네임스페이스는 프로세스가 어떤 마운트 지점을 볼 수 있는지를 제어하지만, `flock`은 inode 자체에 대해 동작합니다. 두 프로세스가 서로 다른 마운트 경로를 통해 같은 inode를 보고 있다면 여전히 충돌합니다.

## The Solution

`private_paths` Coastfile 필드는 워크스페이스 기준으로 인스턴스별이어야 하는 디렉터리를 선언합니다. 각 Coast 인스턴스는 이러한 경로에 대해 자체적으로 격리된 바인드 마운트를 가지며, 이는 컨테이너 자체 파일시스템의 인스턴스별 디렉터리를 기반으로 합니다.

```toml
[coast]
name = "my-app"
private_paths = ["frontend/.next"]
```

Coast가 공유 전파가 설정된 `/workspace`를 마운트한 뒤, 각 private path에 대해 추가 바인드 마운트를 적용합니다.

```text
mkdir -p /coast-private/frontend/.next /workspace/frontend/.next
mount --bind /coast-private/frontend/.next /workspace/frontend/.next
```

`/coast-private/`는 공유된 호스트 바인드 마운트가 아니라 DinD 컨테이너의 쓰기 가능한 레이어에 존재하므로, 각 인스턴스는 자연스럽게 서로 다른 inode를 갖게 됩니다. `dev-1`의 락 파일은 `dev-2`의 락 파일과 다른 inode에 존재하게 되며, 충돌은 사라집니다.

## How It Works

Private path 마운트는 `/workspace`가 마운트되거나 다시 마운트되는 Coast 라이프사이클의 모든 지점에서 적용됩니다:

1. **`coast run`** — 초기 `mount --bind /host-project /workspace && mount --make-rshared /workspace` 이후 private path가 마운트됩니다.
2. **`coast start`** — 컨테이너 재시작 시 워크스페이스 바인드 마운트를 다시 적용한 이후.
3. **`coast assign`** — `/workspace`를 언마운트하고 워크트리 디렉터리로 다시 바인드한 이후.
4. **`coast unassign`** — `/workspace`를 프로젝트 루트로 되돌린 이후.

Private 디렉터리는 stop/start 주기 전반에 걸쳐 유지됩니다(공유 마운트가 아니라 컨테이너 파일시스템에 존재하기 때문입니다). `coast assign` 또는 `coast unassign` 시에는 private 디렉터리가 **비워집니다**. 이는 개발 서버가 이전 브랜치의 오래된 빌드 출력물을 제공하는 대신 올바른 브랜치의 소스 파일로 다시 컴파일하도록 하기 위함입니다. `coast rm` 시에는 컨테이너와 함께 삭제됩니다.

## When to Use It

도구가 동시 실행되는 Coast 인스턴스 간에 충돌하는 프로세스별 또는 인스턴스별 상태를 워크스페이스 디렉터리에 기록하는 경우 `private_paths`를 사용하세요:

- **파일 락**: `.next/dev/lock`, Cargo의 `target/.cargo-lock`, Gradle의 `.gradle/lock`
- **빌드 캐시**: `.next`, `.turbo`, `target/`, `.vite`
- **PID 파일**: 워크스페이스에 PID 파일을 기록하는 모든 데몬

인스턴스 간 공유되어야 하거나 호스트에서 보여야 하는 데이터에는 `private_paths`를 사용하지 마세요. 지속적이고 Docker가 관리하는 격리된 데이터(예: 데이터베이스 볼륨)가 필요하다면 대신 [volumes with `strategy = "isolated"`](../coastfiles/VOLUMES.md)를 사용하세요.

## Validation Rules

- 경로는 상대 경로여야 합니다(앞에 `/` 금지)
- 경로에는 `..` 구성 요소가 포함되면 안 됩니다
- 경로는 서로 겹치면 안 됩니다 — `frontend/.next`와 `frontend/.next/cache`를 둘 다 나열하는 것은 오류입니다. 첫 번째 마운트가 두 번째를 가리게 되기 때문입니다

## Relationship to Volumes

`private_paths`와 `[volumes]`는 서로 다른 격리 문제를 해결합니다:

| | `private_paths` | `[volumes]` |
|---|---|---|
| **무엇** | 워크스페이스 기준 디렉터리 | Docker가 관리하는 이름 있는 볼륨 |
| **어디에** | `/workspace` 내부 | 임의의 컨테이너 마운트 경로 |
| **기반 저장소** | 컨테이너 로컬 파일시스템 (`/coast-private/`) | Docker named volumes |
| **격리** | 항상 인스턴스별 | `isolated` 또는 `shared` 전략 |
| **`coast rm` 이후 유지** | 아니요 | Isolated: 아니요. Shared: 예. |
| **사용 사례** | 빌드 산출물, 락 파일, 캐시 | 데이터베이스, 지속적인 애플리케이션 데이터 |

## Configuration Reference

전체 문법과 예시는 Coastfile 레퍼런스의 [`private_paths`](../coastfiles/PROJECT.md)를 참고하세요.
