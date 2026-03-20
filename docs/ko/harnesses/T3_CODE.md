# T3 코드

[T3 Code](https://github.com/pingdotgg/t3code)는 `~/.t3/worktrees/<project-name>/`에 git worktree를 생성하고, 이름이 지정된 브랜치로 체크아웃합니다.

T3 Code에서는 항상 활성화되는 Coast Runtime 규칙을 `AGENTS.md`에 두고, 재사용 가능한 `/coasts` 워크플로를 `.agents/skills/coasts/SKILL.md`에 두세요.

이러한 worktree는 프로젝트 루트 외부에 위치하므로, Coasts가 이를 발견하고 마운트하려면 명시적인 구성이 필요합니다.

## 설정

`~/.t3/worktrees/<project-name>`를 `worktree_dir`에 추가하세요. T3 Code는 프로젝트별 하위 디렉터리 아래에 worktree를 중첩하므로, 경로에는 반드시 프로젝트 이름이 포함되어야 합니다. 아래 예시에서 `my-app`은 `~/.t3/worktrees/` 아래에 있는 저장소의 실제 폴더 이름과 일치해야 합니다.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.t3/worktrees/my-app"]
```

Coasts는 런타임에 `~`를 확장하며, `~/` 또는 `/`로 시작하는 모든 경로를 외부 경로로 처리합니다. 자세한 내용은 [Worktree Directories](../coastfiles/WORKTREE_DIR.md)를 참조하세요.

`worktree_dir`를 변경한 후에는 바인드 마운트가 적용되도록 기존 인스턴스를 반드시 **재생성**해야 합니다:

```bash
coast rm my-instance
coast build
coast run my-instance
```

worktree 목록은 즉시 업데이트되지만(Coasts가 새 Coastfile을 읽음), T3 Code worktree에 할당하려면 컨테이너 내부의 바인드 마운트가 필요합니다.

## Coasts 가이드의 위치

T3 Code에는 다음 레이아웃을 사용하세요:

- 짧은 Coast Runtime 규칙은 `AGENTS.md`에 둡니다
- 재사용 가능한 `/coasts` 워크플로는 `.agents/skills/coasts/SKILL.md`에 둡니다
- Coasts를 위해 별도의 T3 전용 프로젝트 명령 또는 슬래시 명령 계층을 추가하지 마세요
- 이 저장소가 여러 하니스를 사용한다면
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) 및
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md)를 참조하세요.

## Coasts가 수행하는 작업

- **실행** — `coast run <name>`은 최신 빌드에서 새 Coast 인스턴스를 생성합니다. `coast run <name> -w <worktree>`를 사용하면 T3 Code worktree를 생성하고 할당하는 작업을 한 번에 수행할 수 있습니다. 자세한 내용은 [Run](../concepts_and_terminology/RUN.md)을 참조하세요.
- **바인드 마운트** — 컨테이너 생성 시, Coasts는 `~/.t3/worktrees/<project-name>`를 컨테이너 내부의 `/host-external-wt/{index}`에 마운트합니다.
- **발견** — `git worktree list --porcelain`는 저장소 범위로 동작하므로, 현재 프로젝트에 속한 worktree만 표시됩니다.
- **이름 지정** — T3 Code worktree는 이름이 지정된 브랜치를 사용하므로, Coasts UI와 CLI에서 브랜치 이름으로 표시됩니다.
- **할당** — `coast assign`은 외부 바인드 마운트 경로에서 `/workspace`를 다시 마운트합니다.
- **gitignored 동기화** — 절대 경로를 사용해 호스트 파일시스템에서 실행되므로, 바인드 마운트 없이도 작동합니다.
- **고아 감지** — git watcher는 `.git` gitdir 포인터를 기준으로 필터링하면서 외부 디렉터리를 재귀적으로 스캔합니다. T3 Code가 워크스페이스를 제거하면 Coasts는 인스턴스 할당을 자동으로 해제합니다.

## 예시

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.t3/worktrees/my-app"]
primary_port = "web"

[ports]
web = 3000
api = 8080

[assign]
default = "none"
[assign.services]
web = "hot"
api = "hot"
```

- `.claude/worktrees/` — Claude Code (로컬, 특별한 처리 없음)
- `~/.codex/worktrees/` — Codex (외부, 바인드 마운트됨)
- `~/.t3/worktrees/my-app/` — T3 Code (외부, 바인드 마운트됨; `my-app`을 저장소 폴더 이름으로 바꾸세요)

## 제한 사항

- Coasts 내부의 런타임 구성에 T3 Code 전용 환경 변수를 사용하는 것에 의존하지 마세요. Coasts는 포트, 워크스페이스 경로, 서비스 디스커버리를 독립적으로 관리하므로, 대신 Coastfile `[ports]`와 `coast exec`를 사용하세요.
