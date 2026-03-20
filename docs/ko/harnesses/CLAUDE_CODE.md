# Claude Code

[Claude Code](https://docs.anthropic.com/en/docs/claude-code/overview)는
프로젝트 내부의 `.claude/worktrees/`에 worktree를 생성합니다. 그 디렉터리는
리포지토리 내부에 있으므로, Coasts는 외부 bind mount 없이도 Claude Code worktree를
발견하고 할당할 수 있습니다.

여기서 Claude Code는 Coasts를 위한 세 가지 계층이 가장 명확하게 분리된
하네스이기도 합니다:

- Coasts와 함께 작업할 때의 짧고 항상 적용되는 규칙을 위한 `CLAUDE.md`
- 재사용 가능한 `/coasts` 워크플로를 위한 `.claude/skills/coasts/SKILL.md`
- 추가 진입점으로 명령 파일이 필요할 때만 사용하는 `.claude/commands/coasts.md`

## Setup

`.claude/worktrees`를 `worktree_dir`에 추가하세요:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

`.claude/worktrees`는 프로젝트 기준 상대 경로이므로, 외부 bind mount는
필요하지 않습니다.

## Where Coasts guidance goes

### `CLAUDE.md`

모든 작업에 적용되어야 하는 Coasts 규칙은 여기에 두세요. 짧고 실행 중심으로
유지하세요:

- 세션에서 첫 번째 런타임 명령 전에 `coast lookup` 실행
- 테스트, 빌드, 서비스 명령에는 `coast exec` 사용
- 런타임 피드백에는 `coast ps` 및 `coast logs` 사용
- 일치하는 Coast가 없을 때 Coast를 생성하거나 재할당하기 전에 질문

### `.claude/skills/coasts/SKILL.md`

재사용 가능한 `/coasts` 워크플로는 여기에 두세요. 다음과 같은 흐름에 적합한
위치입니다:

1. `coast lookup`을 실행하고 일치하는 Coast를 재사용
2. 일치하는 항목이 없으면 `coast ls`로 대체
3. `coast run`, `coast assign`, `coast unassign`, `coast checkout`, `coast ui`를 제안
4. 래핑하지 않고 Coast CLI 자체를 계약으로 사용

이 리포지토리에서 Codex, T3 Code 또는 Cursor도 사용한다면,
[Multiple Harnesses](MULTIPLE_HARNESSES.md)를 참고하고 정식 skill은
`.agents/skills/coasts/`에 유지한 다음 Claude Code에 노출하세요.

### `.claude/commands/coasts.md`

Claude Code는 프로젝트 명령 파일도 지원합니다. Coasts에 대한 문서에서는
이를 선택 사항으로 취급하세요:

- 명령 파일이 특별히 필요할 때만 사용
- 한 가지 단순한 옵션은 명령이 같은 skill을 재사용하게 하는 것
- 명령에 별도의 자체 지침을 부여하면, 유지해야 할 워크플로의 두 번째 복사본을
  떠안게 됩니다

## Example layout

### Claude Code only

```text
CLAUDE.md
.claude/worktrees/
.claude/skills/coasts/SKILL.md
```

이 리포지토리에서 Codex, T3 Code 또는 Cursor도 사용한다면, 여기서 이를
중복하지 말고 [Multiple Harnesses](MULTIPLE_HARNESSES.md)의 공유 패턴을
사용하세요. 제공자별 지침이 중복되면 새 하네스를 추가할 때마다 동기화 상태를
유지하기가 더 어려워지기 때문입니다.

## What Coasts does

- **실행** — `coast run <name>`은 최신 빌드에서 새로운 Coast 인스턴스를 생성합니다. `coast run <name> -w <worktree>`를 사용하면 Claude Code worktree를 생성하고 한 번에 할당할 수 있습니다. [Run](../concepts_and_terminology/RUN.md)을 참고하세요.
- **검색** — Coasts는 다른 로컬 worktree 디렉터리와 마찬가지로 `.claude/worktrees`를 읽습니다.
- **이름 지정** — Claude Code worktree는 Coasts UI와 CLI에서 다른 리포지토리 내부 worktree와 동일한 로컬 worktree 이름 지정 동작을 따릅니다.
- **할당** — `coast assign`은 외부 bind-mount 우회 없이 `/workspace`를 Claude Code worktree로 전환할 수 있습니다.
- **Gitignored 동기화** — worktree가 리포지토리 트리 내부에 있으므로 정상적으로 작동합니다.
- **고아 감지** — Claude Code가 worktree를 제거하면, Coasts는 누락된 gitdir를 감지하고 필요할 때 할당을 해제할 수 있습니다.

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
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

- `.claude/worktrees/` — Claude Code worktree
- `~/.codex/worktrees/` — 이 리포지토리에서 Codex도 사용하는 경우의 Codex worktree

## Limitations

- `CLAUDE.md`, `.claude/skills`, `.claude/commands` 전반에 동일한 `/coasts`
  워크플로를 중복하면, 그 복사본들은 서로 어긋나게 됩니다. `CLAUDE.md`는 짧게
  유지하고 재사용 가능한 워크플로는 하나의 skill에만 두세요.
- 하나의 리포지토리가 여러 하네스에서 깔끔하게 작동하길 원한다면,
  [Multiple Harnesses](MULTIPLE_HARNESSES.md)의 공유 패턴을 우선하세요.
