# 워크트리 디렉터리

`[coast]`의 `worktree_dir` 필드는 git 워크트리가 위치할 곳을 제어합니다. Coast는 git 워크트리를 사용하여 전체 저장소를 복제하지 않고도 각 인스턴스가 서로 다른 브랜치에서 코드베이스의 자체 복사본을 갖도록 합니다.

## 구문

`worktree_dir`는 단일 문자열 또는 문자열 배열을 받을 수 있습니다:

```toml
# Single directory (default)
worktree_dir = ".worktrees"

# Multiple directories
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

생략하면 기본값은 `".worktrees"`입니다.

## 경로 유형

### 상대 경로

`~/` 또는 `/`로 시작하지 않는 경로는 프로젝트 루트를 기준으로 해석됩니다. 이것이 가장 일반적인 경우이며 별도의 특별한 처리가 필요 없습니다 — 프로젝트 디렉터리 내부에 있으므로 표준 `/host-project` 바인드 마운트를 통해 Coast 컨테이너 내부에서 자동으로 사용할 수 있습니다.

```toml
worktree_dir = ".worktrees"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

### 물결표 경로(외부)

`~/`로 시작하는 경로는 사용자의 홈 디렉터리로 확장되며 **외부** 워크트리 디렉터리로 취급됩니다. Coast는 컨테이너가 해당 경로에 접근할 수 있도록 별도의 바인드 마운트를 추가합니다.

```toml
worktree_dir = ["~/.codex/worktrees", ".worktrees"]
```

이것은 OpenAI Codex처럼 프로젝트 루트 외부에 워크트리를 생성하는 도구와 통합하는 방법입니다(Codex는 항상 `$CODEX_HOME/worktrees`에 워크트리를 생성합니다).

### 절대 경로(외부)

`/`로 시작하는 경로도 외부로 취급되며 자체 바인드 마운트를 갖습니다.

```toml
worktree_dir = ["/shared/worktrees", ".worktrees"]
```

## 외부 디렉터리가 작동하는 방식

Coast가 외부 워크트리 디렉터리(물결표 또는 절대 경로)를 만나면 세 가지 일이 발생합니다:

1. **컨테이너 바인드 마운트** — 컨테이너 생성 시점(`coast run`)에 해석된 호스트 경로가 `/host-external-wt/{index}`에 바인드 마운트되며, 여기서 `{index}`는 `worktree_dir` 배열에서의 위치입니다. 이렇게 하면 컨테이너 내부에서 외부 파일에 접근할 수 있습니다.

2. **프로젝트 필터링** — 외부 디렉터리에는 여러 프로젝트의 워크트리가 포함될 수 있습니다. Coast는 `git worktree list --porcelain`(본질적으로 현재 저장소 범위로 제한됨)을 사용하여 이 프로젝트에 속한 워크트리만 찾습니다. git watcher는 또한 각 워크트리의 `.git` 파일을 읽고 그 `gitdir:` 포인터가 현재 저장소로 다시 해석되는지 확인하여 소유권을 검증합니다.

3. **워크스페이스 리마운트** — 외부 워크트리에 `coast assign`하면 Coast는 일반적인 `/host-project/{dir}/{name}` 대신 외부 바인드 마운트 경로에서 `/workspace`를 다시 마운트합니다.

## 외부 워크트리의 이름 지정

브랜치가 체크아웃된 외부 워크트리는 로컬 워크트리와 동일하게 브랜치 이름으로 표시됩니다.

**detached HEAD** 상태의 외부 워크트리(Codex에서 흔함)는 외부 디렉터리 내 상대 경로를 사용해 표시됩니다. 예를 들어 `~/.codex/worktrees/a0db/coastguard-platform`에 있는 Codex 워크트리는 UI와 CLI에서 `a0db/coastguard-platform`으로 표시됩니다.

## `default_worktree_dir`

Coast가 **새로운** 워크트리를 생성할 때 어떤 디렉터리를 사용할지 제어합니다(예: 기존 워크트리가 없는 브랜치를 할당할 때). 기본값은 `worktree_dir`의 첫 번째 항목입니다.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
default_worktree_dir = ".worktrees"
```

새 워크트리를 생성할 때는 외부 디렉터리를 절대 사용하지 않습니다 — Coast는 항상 로컬(상대) 디렉터리에 워크트리를 생성합니다. `default_worktree_dir` 필드는 기본값(첫 번째 항목)을 재정의하려는 경우에만 필요합니다.

## 예시

### Codex 통합

OpenAI Codex는 `~/.codex/worktrees/{hash}/{project-name}`에 워크트리를 생성합니다. Coast에서 이것들이 보이고 할당 가능하도록 하려면:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

이것을 추가하면 Codex의 워크트리가 체크아웃 모달과 `coast ls` 출력에 나타납니다. Coast 인스턴스를 Codex 워크트리에 할당하여 전체 개발 환경에서 해당 코드를 실행할 수 있습니다.

참고: 외부 디렉터리를 추가한 후 바인드 마운트가 적용되려면 컨테이너를 다시 생성해야 합니다(`coast run`). 기존 인스턴스를 재시작하는 것만으로는 충분하지 않습니다.

### Claude Code 통합

Claude Code는 프로젝트 내부의 `.claude/worktrees/`에 워크트리를 생성합니다. 이것은 상대 경로이므로(프로젝트 루트 내부) 다른 로컬 워크트리 디렉터리처럼 작동합니다 — 외부 마운트가 필요하지 않습니다:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

### 세 가지를 모두 함께 사용

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

## 라이브 Coastfile 읽기

Coastfile의 `worktree_dir` 변경 사항은 워크트리 **목록 표시**에 즉시 반영됩니다(API와 git watcher는 캐시된 빌드 아티팩트만이 아니라 디스크의 라이브 Coastfile을 읽습니다). 그러나 외부 **바인드 마운트**는 컨테이너 생성 시점에만 만들어지므로, 새로 추가한 외부 디렉터리를 마운트 가능하게 하려면 인스턴스를 다시 생성해야 합니다.
