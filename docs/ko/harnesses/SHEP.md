# Shep

## 빠른 설정

[Coast CLI](../GETTING_STARTED.md)가 필요합니다. Coasts를 자동으로 설정하려면 이 프롬프트를 에이전트의 채팅에 복사하세요:

```prompt-copy
shep_setup_prompt.txt
```

CLI에서도 스킬 내용을 가져올 수 있습니다: `coast skills-prompt`.

설정 후에는 새 스킬과 프로젝트 지침이 적용되도록 **에디터를 종료했다가 다시 여세요**.

---

[Shep](https://shep-ai.github.io/cli/)는 `~/.shep/repos/{hash}/wt/{branch-slug}`에 worktree를 생성합니다. 해시는 저장소 절대 경로의 SHA-256 앞 16자리 16진수 문자이며, 따라서 저장소별로 결정적이지만 식별하기는 어렵습니다. 주어진 저장소의 모든 worktree는 동일한 해시를 공유하고 `wt/{branch-slug}` 하위 디렉터리로 구분됩니다.

Shep CLI에서 `shep feat show <feature-id>`는 worktree 경로를 출력하며, 또는
`ls ~/.shep/repos`로 저장소별 해시 디렉터리를 나열할 수 있습니다.

해시는 저장소마다 달라지므로, Coasts는 사용자가 해시를 하드코딩할 필요 없이
shep worktree를 찾기 위해 **glob 패턴**을 사용합니다.

## 설정

`~/.shep/repos/*/wt`를 `worktree_dir`에 추가하세요:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
```

`*`는 저장소별 해시 디렉터리와 일치합니다. 런타임에 Coasts는 glob을 확장하고,
일치하는 디렉터리(예: `~/.shep/repos/a21f0cda9ab9d456/wt`)를 찾은 다음 이를
컨테이너에 bind mount합니다. glob 패턴에 대한 전체 자세한 내용은
[Worktree Directories](../coastfiles/WORKTREE_DIR.md)를 참조하세요.

`worktree_dir`를 변경한 후에는 bind mount가 적용되도록 기존 인스턴스를 **다시 생성**해야 합니다:

```bash
coast rm my-instance
coast build
coast run my-instance
```

worktree 목록은 즉시 업데이트됩니다(Coasts가 새 Coastfile을 읽기 때문). 하지만
Shep worktree에 할당하려면 컨테이너 내부의 bind mount가 필요합니다.

## Coasts 지침이 들어가는 위치

Shep는 내부적으로 Claude Code를 감싸므로, Claude Code 규칙을 따르세요:

- 짧은 Coast Runtime 규칙은 `CLAUDE.md`에 둡니다
- 재사용 가능한 `/coasts` 워크플로는 `.claude/skills/coasts/SKILL.md` 또는
  공유 `.agents/skills/coasts/SKILL.md`에 둡니다
- 이 저장소가 다른 harness도 함께 사용한다면,
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) 및
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md)를 참조하세요

## Coasts가 하는 일

- **실행** -- `coast run <name>`은 최신 빌드에서 새 Coast 인스턴스를 생성합니다. `coast run <name> -w <worktree>`를 사용하면 Shep worktree를 생성하고 할당하는 작업을 한 번에 할 수 있습니다. [Run](../concepts_and_terminology/RUN.md)을 참조하세요.
- **Bind mount** -- 컨테이너 생성 시 Coasts는 glob
  `~/.shep/repos/*/wt`를 해석하고 일치하는 각 디렉터리를 컨테이너의
  `/host-external-wt/{index}`에 마운트합니다.
- **탐색** -- `git worktree list --porcelain`는 저장소 범위로 작동하므로
  현재 프로젝트에 속한 worktree만 표시됩니다.
- **이름 지정** -- Shep worktree는 이름 있는 브랜치를 사용하므로 Coasts UI와 CLI에
  브랜치 이름(예: `feat-green-background`)으로 표시됩니다.
- **할당** -- `coast assign`은 `/workspace`를 외부 bind mount 경로에서 다시 마운트합니다.
- **gitignored 동기화** -- 호스트 파일시스템에서 절대 경로로 실행되며 bind mount 없이도 작동합니다.
- **고아 감지** -- git watcher는 외부 디렉터리를 재귀적으로 스캔하면서
  `.git` gitdir 포인터로 필터링합니다. Shep가 worktree를 삭제하면
  Coasts는 인스턴스 할당을 자동 해제합니다.

## 예시

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
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

- `~/.shep/repos/*/wt` -- Shep (외부, glob 확장을 통한 bind-mounted)

## Shep 경로 구조

```
~/.shep/repos/
  {sha256-of-repo-path-first-16-chars}/
    wt/
      {branch-slug}/     <-- git worktree
      {branch-slug}/
```

핵심 사항:
- 같은 저장소 = 항상 같은 해시(결정적이며 무작위가 아님)
- 다른 저장소 = 다른 해시
- 경로 구분자는 해싱 전에 `/`로 정규화됨
- 해시는 `shep feat show <feature-id>` 또는 `ls ~/.shep/repos`로 찾을 수 있음
