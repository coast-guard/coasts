# 할당 및 할당 해제

할당(assign)과 할당 해제(unassign)는 Coast 인스턴스가 어떤 worktree를 가리키는지 제어합니다. 마운트 레벨에서 worktree 전환이 어떻게 동작하는지에 대해서는 [Filesystem](FILESYSTEM.md)을 참고하세요.

## 할당

`coast assign`은 Coast 인스턴스를 특정 worktree로 전환합니다. Coast는 worktree가 아직 존재하지 않으면 이를 생성하고, Coast 내부의 코드를 업데이트하며, 구성된 할당 전략(assign strategy)에 따라 서비스를 재시작합니다.

```bash
coast assign dev-1 --worktree feature/oauth
```

```text
Before:
┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘

coast assign dev-1 --worktree feature/oauth

After:
┌─── dev-1 ──────────────────┐
│  branch: feature/oauth     │
│  worktree: feature/oauth   │
│                            │
│  postgres → skipped (none) │
│  web      → hot swapped    │
│  api      → restarted      │
│  worker   → rebuilt        │
└────────────────────────────┘
```

할당 후 `dev-1`은 `feature/oauth` 브랜치를 실행하며, 모든 서비스가 올라온 상태입니다.

## 할당 해제

`coast unassign`은 Coast 인스턴스를 프로젝트 루트(메인/마스터 브랜치)로 되돌립니다. worktree 연결이 제거되고 Coast는 기본 리포지토리에서 실행 상태로 돌아갑니다.

```text
coast unassign dev-1

┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘
```

## 할당 전략

Coast가 새 worktree에 할당되면, 각 서비스는 코드 변경을 어떻게 처리할지 알아야 합니다. 이는 [Coastfile](COASTFILE_TYPES.md)의 `[assign]` 아래에서 서비스별로 구성합니다:

```toml
[assign]
default = "restart"

[assign.services]
postgres = "none"
redis = "none"
web = "hot"
worker = "rebuild"
```

```text
coast assign dev-1 --worktree feature/billing

  postgres (strategy: none)    →  skipped, unchanged between branches
  redis (strategy: none)       →  skipped, unchanged between branches
  web (strategy: hot)          →  filesystem swapped, file watcher picks it up
  api (strategy: restart)      →  container restarted
  worker (strategy: rebuild)   →  image rebuilt, container restarted
```

사용 가능한 전략은 다음과 같습니다:

- **none** — 아무것도 하지 않습니다. Postgres나 Redis처럼 브랜치 간에 변경되지 않는 서비스에 사용하세요.
- **hot** — 파일시스템만 스왑합니다. 서비스는 계속 실행되며 마운트 전파와 파일 워처(예: 핫 리로드가 있는 개발 서버)를 통해 변경 사항을 반영합니다.
- **restart** — 서비스 컨테이너를 재시작합니다. 프로세스 재시작만 필요로 하는 인터프리터 기반 서비스에 사용하세요. 기본값입니다.
- **rebuild** — 서비스 이미지를 다시 빌드하고 재시작합니다. 브랜치 변경이 `Dockerfile` 또는 빌드 타임 의존성에 영향을 줄 때 사용하세요.

또한 특정 파일이 변경될 때만 서비스가 리빌드하도록 리빌드 트리거를 지정할 수 있습니다:

```toml
[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
```

브랜치 간에 트리거 파일 중 어느 것도 변경되지 않았다면, 전략이 `rebuild`로 설정되어 있어도 해당 서비스는 리빌드를 건너뜁니다.

## 삭제된 Worktree

할당된 worktree가 삭제되면 `coastd` 데몬이 해당 인스턴스를 자동으로 메인 Git 리포지토리 루트로 할당 해제합니다.

---

> **팁: 대규모 코드베이스에서 할당 지연 시간 줄이기**
>
> 내부적으로 Coast는 worktree가 마운트되거나 언마운트될 때마다 `git ls-files`를 실행합니다. 대규모 코드베이스나 파일이 많은 리포지토리에서는 이것이 할당 및 할당 해제 작업에 눈에 띄는 지연 시간을 추가할 수 있습니다.
>
> 코드베이스의 일부가 할당 간에 재빌드될 필요가 없다면, Coastfile의 `exclude_paths`를 사용하여 이를 건너뛰도록 Coast에 알려줄 수 있습니다:
>
> ```toml
> [assign]
> default = "restart"
> exclude_paths = ["docs", "scripts", "test-fixtures"]
> ```
>
> `exclude_paths`에 나열된 경로는 파일 diff 동안 무시되며, 이는 할당 시간을 크게 단축할 수 있습니다.
