# 할당 및 할당 해제

할당과 할당 해제는 Coast 인스턴스가 어떤 worktree를 가리키는지 제어합니다. 마운트 레벨에서 worktree 전환이 어떻게 동작하는지에 대해서는 [Filesystem](FILESYSTEM.md)을 참고하세요.

## 할당

`coast assign`은 Coast 인스턴스를 특정 worktree로 전환합니다. Coast는 해당 worktree가 아직 존재하지 않으면 생성하고, Coast 내부의 코드를 업데이트한 다음, 구성된 할당 전략에 따라 서비스를 재시작합니다.

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

할당 후에는 `dev-1`이 `feature/oauth` 브랜치를 실행하며 모든 서비스가 올라와 있습니다.

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

Coast가 새 worktree에 할당될 때, 각 서비스는 코드 변경을 어떻게 처리할지 알아야 합니다. 이는 [Coastfile](COASTFILE_TYPES.md)의 `[assign]` 아래에서 서비스별로 구성합니다:

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
- **hot** — 파일시스템만 교체합니다. 서비스는 계속 실행 중이며 마운트 전파와 파일 워처를 통해 변경을 반영합니다(예: 핫 리로드가 있는 개발 서버).
- **restart** — 서비스 컨테이너를 재시작합니다. 프로세스 재시작만 필요로 하는 인터프리터 기반 서비스에 사용하세요. 이것이 기본값입니다.
- **rebuild** — 서비스 이미지를 다시 빌드한 뒤 재시작합니다. 브랜치 변경이 `Dockerfile` 또는 빌드 타임 의존성에 영향을 줄 때 사용하세요.

또한 특정 파일이 변경될 때만 서비스가 rebuild되도록 rebuild 트리거를 지정할 수 있습니다:

```toml
[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
```

브랜치 간에 트리거 파일이 하나도 변경되지 않았다면, 전략이 `rebuild`로 설정되어 있더라도 서비스는 rebuild를 건너뜁니다.

## 삭제된 Worktree

할당된 worktree가 삭제되면, `coastd` 데몬이 해당 인스턴스를 자동으로 할당 해제하여 메인 Git 리포지토리 루트로 되돌립니다.

---

> **팁: 대규모 코드베이스에서 할당 지연 시간 줄이기**
>
> 내부적으로, 새로운 worktree에 대한 첫 할당은 선택된 gitignored 파일들을 해당 worktree로 부트스트랩하며, `[assign.rebuild_triggers]`가 있는 서비스는 rebuild가 필요한지 결정하기 위해 `git diff --name-only`를 실행할 수 있습니다. 대규모 코드베이스에서는 이 부트스트랩 단계와 불필요한 rebuild가 할당 시간을 지배하는 경향이 있습니다.
>
> Coastfile의 `exclude_paths`를 사용해 gitignored 부트스트랩 범위를 줄이고, 파일 워처가 있는 서비스에는 `"hot"`을 사용하며, `[assign.rebuild_triggers]`는 진짜 빌드 타임 입력에만 집중시키세요. 기존 worktree에 대해 ignored-file 부트스트랩을 수동으로 새로 고쳐야 한다면 `coast assign --force-sync`를 실행하세요. 전체 가이드는 [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md)을 참고하세요.
