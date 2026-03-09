# 성능 최적화

Coast는 브랜치 전환을 빠르게 만들도록 설계되었지만, 대규모 모노레포에서는 기본 동작만으로도 지연이 발생할 수 있습니다. 이 페이지에서는 Coastfile에서 사용할 수 있는 조절 장치들과, 더 중요하게는 그것들이 실제로 `coast assign`의 어떤 부분에 영향을 미치는지 다룹니다.

## Assign이 느릴 수 있는 이유

`coast assign`는 Coast를 새 워크트리로 전환할 때 여러 작업을 수행합니다:

```text
coast assign dev-1 --worktree feature/payments

  1. classify services and optional rebuild-trigger diff
  2. stop affected services
  3. create git worktree (if new)
  4. bootstrap gitignored files into the worktree (first assign only)
  5. remount /workspace
  6. recreate/restart containers
  7. rebuild images for services using "rebuild"
  8. wait for healthy
```

가장 큰 변동 비용은 보통 **최초 gitignored 부트스트랩**, **컨테이너 재시작**, **이미지 리빌드**입니다. 리빌드 트리거를 위한 선택적 브랜치 diff는 훨씬 저렴하지만, 트리거 세트를 광범위하게 지정하면 이것도 누적될 수 있습니다.

### Gitignored 파일 부트스트랩

워크트리가 처음 생성될 때, Coast는 프로젝트 루트에서 선택된 gitignored 파일을 해당 워크트리로 부트스트랩합니다.

순서는 다음과 같습니다:

1. 호스트에서 `git ls-files --others --ignored --exclude-standard`를 실행해 무시된 파일을 열거합니다.
2. 일반적으로 무거운 디렉터리들과 설정된 `exclude_paths`를 필터링합니다.
3. 선택된 파일이 바이트 단위로 복사되는 대신 워크트리에 하드링크되도록 `--link-dest`와 함께 `rsync --files-from`를 실행합니다.
4. 동일한 워크트리에 대한 이후 assign에서 이를 건너뛸 수 있도록, 내부 워크트리 메타데이터에 성공한 부트스트랩을 기록합니다.

`rsync`를 사용할 수 없으면, Coast는 `tar` 파이프라인으로 대체합니다.

`node_modules`, `.git`, `dist`, `target`, `.next`, `.nuxt`, `.cache`, `.worktrees`, `.coasts` 같은 대규모 디렉터리는 자동으로 제외됩니다. 큰 의존성 디렉터리는 이 범용 부트스트랩 단계가 아니라 서비스 캐시나 볼륨으로 처리되는 것이 기대됩니다.

파일 목록은 사전에 생성되므로, `rsync`는 저장소 전체를 무작정 크롤링하는 대신 대상이 지정된 목록을 기반으로 동작합니다. 그럼에도 매우 큰 ignored-file 집합을 가진 저장소는 워크트리가 처음 생성될 때 눈에 띄는 일회성 부트스트랩 비용을 치를 수 있습니다. 이 부트스트랩을 수동으로 새로 고쳐야 한다면 `coast assign --force-sync`를 실행하세요.

### Rebuild-Trigger Diff

Coast는 `[assign.rebuild_triggers]`가 구성된 경우에만 브랜치 diff를 계산합니다. 그 경우 다음을 실행합니다:

```bash
git diff --name-only <previous>..<worktree>
```

그 결과는 트리거 파일이 하나도 변경되지 않았을 때 서비스를 `rebuild`에서 `restart`로 다운그레이드하는 데 사용됩니다.

이는 예전의 “매 assign마다 추적된 모든 파일을 diff”하는 모델보다 훨씬 좁은 범위입니다. 리빌드 트리거를 구성하지 않으면, 여기에는 브랜치 diff 단계가 아예 없습니다.

`exclude_paths`는 현재 이 diff를 변경하지 않습니다. 트리거 목록은 Dockerfile, lockfile, 패키지 매니페스트 같은 진정한 빌드 시점 입력에 집중해서 유지하세요.

## `exclude_paths` — 새 워크트리를 위한 주요 레버

Coastfile의 `exclude_paths` 옵션은 새 워크트리를 위한 gitignored 부트스트랩 파일 목록을 만드는 동안 전체 디렉터리 트리를 건너뛰도록 Coast에 지시합니다.

```toml
[assign]
default = "none"
exclude_paths = [
    "docs",
    "scripts",
    "test-fixtures",
    "apps/mobile",
]
```

제외된 경로 아래의 파일이 Git에 의해 추적되는 경우, 그 파일들은 여전히 워크트리에 존재합니다. Coast는 최초 부트스트랩 동안 해당 트리 아래의 무시된 파일을 열거하고 하드링크하는 데 시간을 쓰지 않을 뿐입니다.

이는 저장소 루트에 실행 중인 서비스가 신경 쓰지 않는 큰 ignored 디렉터리가 있을 때 가장 효과가 큽니다: 관련 없는 앱, 벤더된 캐시, 테스트 픽스처, 생성된 문서, 기타 무거운 트리 등.

이미 동기화된 동일한 워크트리에 반복적으로 assign하는 경우에는 부트스트랩이 건너뛰어지므로 `exclude_paths`의 영향이 줄어듭니다. 그 경우에는 서비스 restart/rebuild 선택이 지배적인 요인이 됩니다.

### 무엇을 제외할지 선택하기

먼저 ignored 파일을 프로파일링하세요:

```bash
git ls-files --others --ignored --exclude-standard | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

rebuild-trigger 튜닝을 위해 추적된 레이아웃도 보고 싶다면 다음을 사용하세요:

```bash
git ls-files | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

**유지**해야 할 디렉터리:
- 실행 중인 서비스에 마운트되는 소스 코드를 포함하는 디렉터리
- 해당 서비스들이 임포트하는 공유 라이브러리를 포함하는 디렉터리
- 런타임이 첫 부팅 시 실제로 필요로 하는 생성 파일 또는 캐시를 포함하는 디렉터리
- `[assign.rebuild_triggers]`에서 참조되는 디렉터리

**제외**해야 할 디렉터리:
- 이 Coast에서 실행되지 않는 앱 또는 서비스에 속한 디렉터리
- 런타임과 무관한 문서, 스크립트, CI 설정, 툴링을 포함하는 디렉터리
- 전용 서비스 캐시나 공유 볼륨 등 다른 곳에서 이미 보존되는 큰 ignored 캐시를 담고 있는 디렉터리

### 예: 여러 앱이 있는 모노레포

최상위 디렉터리는 많지만, 이 Coast에서 실행 중인 서비스에 중요한 것은 일부뿐인 모노레포:

```text
  13,000  bookface/         ← active
   7,000  ycinternal/       ← active
     850  shared/           ← used by both
   3,800  .yarn/            ← excludable
   2,500  startupschool/    ← excludable
     500  misc/             ← excludable
     300  ycapp/            ← excludable
     ...  (12 more dirs)    ← excludable
```

```toml
[assign]
default = "none"
exclude_paths = [
    ".yarn",
    "startupschool",
    "misc",
    "ycapp",
    "apply",
    "cli",
    "deploy",
    "lambdas",
    # ... any other directories not needed by active services
]
```

이렇게 하면 최초 워크트리 부트스트랩이 관련 없는 ignored 트리에 시간을 쓰는 대신, 실행 중인 서비스가 실제로 필요로 하는 디렉터리에 집중하게 됩니다.

## `[assign.services]`에서 비활성 서비스를 제거하기

`COMPOSE_PROFILES`가 일부 서비스만 시작한다면, `[assign.services]`에서 비활성 서비스를 제거하세요. Coast는 나열된 모든 서비스에 대해 assign 전략을 평가하며, 실행 중이 아닌 서비스를 재시작하거나 리빌드하는 것은 낭비 작업입니다.

```toml
# Bad — restarts services that aren't running
[assign.services]
web = "restart"
api = "restart"
mobile-api = "restart"   # not in COMPOSE_PROFILES
batch-worker = "restart"  # not in COMPOSE_PROFILES

# Good — only services that are actually running
[assign.services]
web = "restart"
api = "restart"
```

`[assign.rebuild_triggers]`에도 동일하게 적용됩니다 — 활성 상태가 아닌 서비스에 대한 항목을 제거하세요.

## 가능하면 `"hot"` 사용하기

`"hot"` 전략은 컨테이너 재시작을 완전히 건너뜁니다. [파일시스템 리마운트](FILESYSTEM.md)가 `/workspace` 아래의 코드를 교체하면, 서비스의 파일 워처(Vite, webpack, nodemon, air 등)가 변경을 자동으로 감지합니다.

```toml
[assign.services]
web = "hot"        # Vite/webpack dev server with HMR
api = "restart"    # Rails/Go — needs a process restart
```

`"hot"`은 컨테이너 stop/start 사이클을 피하므로 `"restart"`보다 빠릅니다. 파일 워칭이 있는 dev 서버를 실행하는 서비스에는 `"hot"`을 사용하세요. 시작 시 코드를 로드하고 변경을 감시하지 않는 서비스(대부분의 Rails, Go, Java 앱)에는 `"restart"`를 사용하세요.

## 트리거와 함께 `"rebuild"` 사용하기

어떤 서비스의 기본 전략이 `"rebuild"`라면, 브랜치를 전환할 때마다 Docker 이미지가 리빌드됩니다 — 이미지에 영향을 주는 변경이 전혀 없어도 말입니다. 특정 파일에 따라 리빌드를 제한하려면 `[assign.rebuild_triggers]`를 추가하세요:

```toml
[assign.services]
worker = "rebuild"

[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json", "package-lock.json"]
```

브랜치 간에 트리거 파일이 하나도 변경되지 않았다면, Coast는 리빌드를 건너뛰고 대신 재시작으로 폴백합니다. 이는 일상적인 코드 변경에서 비용이 큰 이미지 빌드를 피하게 해줍니다.

## 요약

| 최적화 | 효과 | 영향 대상 | 사용 시점 |
|---|---|---|---|
| `exclude_paths` | 높음 | 최초 gitignored 부트스트랩 | Coast에 필요 없는 큰 ignored 트리가 있는 저장소 |
| 비활성 서비스 제거 | 중간 | 서비스 restart/recreate | `COMPOSE_PROFILES`가 실행 서비스 범위를 제한할 때 |
| `"hot"` 전략 | 높음 | 컨테이너 재시작 | 파일 워처가 있는 서비스(Vite, webpack, nodemon, air) |
| `rebuild_triggers` | 높음 | 이미지 리빌드 + 선택적 브랜치 diff | 인프라 변경에만 `"rebuild"`가 필요한 서비스 |

새 워크트리가 처음 assign될 때 느리다면 `exclude_paths`부터 시작하세요. 반복 assign이 느리다면 `hot` vs `restart`에 집중하고, 비활성 서비스를 정리하며, `rebuild_triggers`를 타이트하게 유지하세요.
