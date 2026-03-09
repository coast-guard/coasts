# 파일시스템

호스트 머신과 모든 Coast 인스턴스는 동일한 프로젝트 파일을 공유합니다. 호스트 프로젝트 루트는 DinD 컨테이너의 `/host-project`에 읽기-쓰기(RW)로 마운트되고, Coast는 활성 작업 트리를 `/workspace`에 바인드 마운트합니다. 이것이 호스트 머신에서 실행 중인 에이전트가 코드를 편집하면 Coast 내부의 서비스가 변경 사항을 실시간으로 반영할 수 있는 이유입니다.

## 공유 마운트

```text
Host machine
│
├── ~/dev/my-app/                     (project root)
│   ├── src/
│   ├── Coastfile
│   ├── docker-compose.yml
│   └── .worktrees/                   (worktrees, gitignored)
│       ├── feature-auth/
│       └── feature-billing/
│
└── Docker daemon (host)
    │
    └── Coast: dev-1 (docker:dind)
        │
        ├── /host-project              ← Docker bind mount of project root (RW, fixed)
        │
        ├── /workspace                 ← mount --bind /host-project (switchable)
        │   ├── src/                     same files, same bytes, instant sync
        │   ├── Coastfile
        │   └── docker-compose.yml
        │
        └── Inner Docker daemon
            └── web service
                └── /app               ← compose bind mount from /workspace/src
```

호스트 프로젝트 루트는 컨테이너가 생성될 때 [DinD 컨테이너](RUNTIMES_AND_SERVICES.md) 내부의 `/host-project`에 읽기-쓰기(RW)로 마운트됩니다. 컨테이너가 시작된 뒤, 컨테이너 내부에서 `mount --bind /host-project /workspace`를 실행하여 공유 마운트 전파(`mount --make-rshared`)가 적용된 작업 경로 `/workspace`를 생성하므로, `/workspace`의 하위 디렉터리를 바인드 마운트하는 내부 compose 서비스들이 올바른 내용을 보게 됩니다.

이 두 단계 접근 방식에는 이유가 있습니다. `/host-project`의 Docker 바인드 마운트는 컨테이너 생성 시점에 고정되며, 컨테이너를 재생성하지 않고는 변경할 수 없습니다. 하지만 컨테이너 내부의 Linux 바인드 마운트인 `/workspace`는 컨테이너 라이프사이클을 건드리지 않고도 언마운트한 뒤 다른 하위 디렉터리(워크트리)로 다시 바인드할 수 있습니다. 이것이 `coast assign`이 빠른 이유입니다.

`/workspace`는 읽기-쓰기(RW)입니다. 파일 변경은 양방향으로 즉시 전파됩니다. 호스트에서 파일을 저장하면 Coast 내부의 개발 서버가 이를 반영합니다. Coast 내부에서 파일을 생성하면 호스트에 나타납니다.

## 호스트 에이전트와 Coast

```text
┌─── Host machine ──────────────────────────────────────────┐
│                                                           │
│   AI Agent (Cursor, Claude Code, etc.)                    │
│     │                                                     │
│     ├── reads/writes files at <project root>/src/         │
│     │       ↕ (instant, same filesystem)                  │
│     ├── coast logs dev-1 --service web --tail 50          │
│     ├── coast ps dev-1                                    │
│     └── coast exec dev-1 -- npm test                      │
│                                                           │
├───────────────────────────────────────────────────────────┤
│                                                           │
│   Coast: dev-1                                            │
│     └── /workspace/src/  ← same bytes as host project/src │
│         └── web service picks up changes on save          │
│                                                           │
└───────────────────────────────────────────────────────────┘
```

파일시스템이 공유되므로, 호스트에서 실행되는 AI 코딩 에이전트는 파일을 자유롭게 편집할 수 있고 Coast 내부에서 실행 중인 서비스는 변경 사항을 즉시 확인합니다. 에이전트는 Coast 컨테이너 내부에서 실행될 필요가 없으며, 평소처럼 호스트에서 동작합니다.

에이전트가 런타임 정보(로그, 서비스 상태, 테스트 출력)가 필요할 때는 호스트에서 Coast CLI 명령을 호출합니다:

- `coast logs dev-1 --service web --tail 50` 서비스 출력(참고: [Logs](LOGS.md))
- `coast ps dev-1` 서비스 상태(참고: [Runtimes and Services](RUNTIMES_AND_SERVICES.md))
- `coast exec dev-1 -- npm test` Coast 내부에서 명령 실행(참고: [Exec & Docker](EXEC_AND_DOCKER.md))

이것이 근본적인 아키텍처상의 장점입니다: **코드 편집은 호스트에서, 런타임은 Coast에서 이루어지며, 공유 파일시스템이 둘을 연결합니다.** 호스트 에이전트는 작업을 수행하기 위해 Coast “안”에 있을 필요가 전혀 없습니다.

## 워크트리 전환

`coast assign`이 Coast를 다른 워크트리로 전환할 때, 프로젝트 루트 대신 해당 git 워크트리를 가리키도록 `/workspace`를 다시 마운트합니다:

```text
coast assign dev-1 --worktree feature-auth

Before:  /workspace  ←──mount──  /host-project                          (project root)
After:   /workspace  ←──mount──  /host-project/.worktrees/feature-auth   (worktree)
```

워크트리는 호스트의 `{project_root}/.worktrees/{worktree_name}`에 생성됩니다. `.worktrees` 디렉터리 이름은 Coastfile의 `worktree_dir`로 설정 가능하며, `.gitignore`에 포함되어야 합니다.

워크트리가 새로 만들어진 경우, Coast는 리마운트 전에 프로젝트 루트에서 선택된 gitignored 파일을 부트스트랩합니다. `git ls-files --others --ignored --exclude-standard`로 무시된 파일을 나열하고, 일반적으로 용량이 큰 디렉터리와 설정된 `exclude_paths`를 제외하도록 필터링한 다음, `--link-dest`와 함께 `rsync --files-from`를 사용해 선택된 파일을 워크트리에 하드링크합니다. Coast는 그 부트스트랩을 내부 워크트리 메타데이터에 기록하며, `coast assign --force-sync`로 명시적으로 새로 고치지 않는 한 동일한 워크트리에 대한 이후 assign에서는 이를 건너뜁니다.

컨테이너 내부에서는 `/workspace`를 lazy-unmount한 뒤 `/host-project/.worktrees/{branch_name}`의 워크트리 하위 디렉터리에 다시 바인드합니다. 이 리마운트는 빠르며 DinD 컨테이너를 재생성하거나 내부 Docker 데몬을 재시작하지 않습니다. 다만 compose 및 bare 서비스는 새 `/workspace`를 통해 바인드 마운트가 해석되도록 리마운트 이후 재생성되거나 재시작될 수 있습니다.

`node_modules`와 같은 대규모 의존성 디렉터리는 이 일반적인 부트스트랩 경로의 일부가 아닙니다. 이런 것들은 보통 서비스별 캐시나 볼륨으로 처리됩니다.

`[assign.rebuild_triggers]`를 사용하는 경우, Coast는 호스트에서 `git diff --name-only <previous>..<worktree>`도 실행하여 `rebuild`로 표시된 서비스를 `restart`로 다운그레이드할 수 있는지 결정합니다. assign 지연 시간에 영향을 주는 세부 사항은 [Assign and Unassign](ASSIGN.md) 및 [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md)를 참고하세요.

`coast unassign`는 `/workspace`를 `/host-project`(프로젝트 루트)로 되돌립니다. 중지 후 `coast start`는 인스턴스에 워크트리가 할당되어 있는지에 따라 올바른 마운트를 다시 적용합니다.

## 모든 마운트

모든 Coast 컨테이너에는 다음 마운트가 있습니다:

| Path | Type | Access | Purpose |
|---|---|---|---|
| `/workspace` | bind mount (in-container) | RW | 프로젝트 루트 또는 워크트리. assign 시 전환 가능. |
| `/host-project` | Docker bind mount | RW | 원본 프로젝트 루트. 컨테이너 생성 시 고정. |
| `/image-cache` | Docker bind mount | RO | `~/.coast/image-cache/`의 사전 풀된 OCI tarball. |
| `/coast-artifact` | Docker bind mount | RO | 재작성된 compose 파일이 포함된 빌드 아티팩트. |
| `/coast-override` | Docker bind mount | RO | [shared services](SHARED_SERVICES.md)를 위한 생성된 compose 오버라이드. |
| `/var/lib/docker` | Named volume | RW | 내부 Docker 데몬 상태. 컨테이너 제거 후에도 유지됨. |

읽기 전용(RO) 마운트는 인프라용입니다 — Coast가 생성하는 빌드 아티팩트, 캐시된 이미지, compose 오버라이드를 담고 있습니다. 사용자는 `coast build`와 Coastfile을 통해 간접적으로 이를 다룹니다. 읽기-쓰기(RW) 마운트는 코드가 위치하는 곳이며 내부 데몬이 상태를 저장하는 곳입니다.
