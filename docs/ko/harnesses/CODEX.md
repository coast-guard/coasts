# Codex

[Codex](https://developers.openai.com/codex/app/worktrees/)는 `$CODEX_HOME/worktrees`(일반적으로 `~/.codex/worktrees`)에 worktree를 생성합니다. 각 worktree는 `~/.codex/worktrees/a0db/project-name`처럼 불투명한 해시 디렉터리 아래에 존재하고, 분리된 HEAD에서 시작하며, Codex의 보존 정책에 따라 자동으로 정리됩니다.

[Codex docs](https://developers.openai.com/codex/app/worktrees/)에서 발췌:

> worktree가 생성되는 위치를 제가 제어할 수 있나요?
> 현재는 불가능합니다. Codex는 일관되게 관리할 수 있도록 `$CODEX_HOME/worktrees` 아래에 worktree를 생성합니다.

이러한 worktree는 프로젝트 루트 밖에 존재하므로, Coasts가 이를 발견하고 마운트하려면 명시적인
구성이 필요합니다.

## Setup

`worktree_dir`에 `~/.codex/worktrees`를 추가하세요:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

Coasts는 런타임에 `~`를 확장하며, `~/` 또는 `/`로 시작하는 모든 경로를
외부 경로로 취급합니다. 자세한 내용은
[Worktree Directories](../coastfiles/WORKTREE_DIR.md)를 참고하세요.

`worktree_dir`를 변경한 후에는 bind mount가 적용되도록 기존 인스턴스를 **다시 생성**해야 합니다:

```bash
coast rm my-instance
coast build
coast run my-instance
```

worktree 목록은 즉시 업데이트됩니다(Coasts가 새 Coastfile을 읽기 때문). 하지만
Codex worktree에 할당하려면 컨테이너 내부의 bind mount가 필요합니다.

## Where Coasts guidance goes

Coasts 작업에는 Codex의 프로젝트 지침 파일과 공유 skill 레이아웃을
사용하세요:

- 짧은 Coast Runtime 규칙은 `AGENTS.md`에 둡니다
- 재사용 가능한 `/coasts` 워크플로는 `.agents/skills/coasts/SKILL.md`에 둡니다
- Codex는 해당 skill을 `/coasts` 명령으로 노출합니다
- Codex 전용 메타데이터를 사용하는 경우, 이를 skill 옆의
  `.agents/skills/coasts/agents/openai.yaml`에 둡니다
- Coasts 관련 문서만을 위한 별도의 프로젝트 명령 파일을 만들지 마세요. skill이
  재사용 가능한 표면입니다
- 이 저장소가 Cursor나 Claude Code도 함께 사용하는 경우, 정본 skill은
  `.agents/skills/`에 두고 그곳에서 노출하세요. 자세한 내용은
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) 및
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md)를 참고하세요.

예를 들어, 최소한의 `.agents/skills/coasts/agents/openai.yaml`은 다음과
같을 수 있습니다:

```yaml
interface:
  display_name: "Coasts"
  short_description: "Inspect, assign, and open Coasts for this repo"
  default_prompt: "Use this skill when the user wants help finding, assigning, or opening a Coast."

policy:
  allow_implicit_invocation: false
```

이렇게 하면 Codex에서 더 보기 좋은 라벨로 skill이 표시되고 `/coasts`가
명시적 명령이 됩니다. skill에 MCP 서버나 기타 OpenAI 관리 도구 연결도
필요한 경우에만 `dependencies.tools`를 추가하세요.

## What Coasts does

- **Run** -- `coast run <name>`은 최신 빌드로부터 새 Coast 인스턴스를 생성합니다. `coast run <name> -w <worktree>`를 사용하면 Codex worktree를 한 단계에서 생성하고 할당할 수 있습니다. 자세한 내용은 [Run](../concepts_and_terminology/RUN.md)을 참고하세요.
- **Bind mount** -- 컨테이너 생성 시, Coasts는
  `~/.codex/worktrees`를 컨테이너 내부의 `/host-external-wt/{index}`에 마운트합니다.
- **Discovery** -- `git worktree list --porcelain`는 리포지토리 범위로 동작하므로, 디렉터리에 많은 프로젝트의 worktree가 들어 있더라도 현재 프로젝트에 속한 Codex worktree만 표시됩니다.
- **Naming** -- 분리된 HEAD worktree는 외부 디렉터리 내 상대 경로(`a0db/my-app`, `eca7/my-app`)로 표시됩니다. 브랜치 기반 worktree는 브랜치 이름으로 표시됩니다.
- **Assign** -- `coast assign`은 외부 bind mount 경로에서 `/workspace`를 다시 마운트합니다.
- **Gitignored sync** -- 절대 경로를 사용해 호스트 파일시스템에서 실행되므로, bind mount 없이도 동작합니다.
- **Orphan detection** -- git watcher는 외부 디렉터리를
  재귀적으로 스캔하면서 `.git` gitdir 포인터로 필터링합니다. Codex가
  worktree를 삭제하면 Coasts는 인스턴스 할당을 자동으로 해제합니다.

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

- `.claude/worktrees/` -- Claude Code(로컬, 특별한 처리 없음)
- `~/.codex/worktrees/` -- Codex(외부, bind mount됨)

## Limitations

- Codex는 언제든지 worktree를 정리할 수 있습니다. Coasts의 orphan detection은
  이를 문제없이 처리합니다.
