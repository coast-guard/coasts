# Cursor

[Cursor](https://cursor.com/docs/agent/overview)는 현재 체크아웃에서 직접 작업할 수 있으며,
Parallel Agents 기능은 `~/.cursor/worktrees/<project-name>/` 아래에 git
worktree를 생성할 수도 있습니다.

Coasts 문서 기준으로는, 이는 두 가지 설정 경우를 의미합니다:

- 현재 체크아웃에서만 Cursor를 사용 중이라면, Cursor 전용
  `worktree_dir` 항목은 필요하지 않습니다
- Cursor Parallel Agents를 사용한다면, Coasts가 해당 worktree를
  발견하고 할당할 수 있도록 Cursor worktree 디렉터리를 `worktree_dir`에 추가하세요

## 설정

### 현재 체크아웃만 사용

Cursor가 이미 열어 둔 체크아웃만 편집하는 경우, Coasts에는 특별한
Cursor 전용 worktree 경로가 필요하지 않습니다. Coasts는 해당 체크아웃을
다른 로컬 저장소 루트와 동일하게 처리합니다.

### Cursor Parallel Agents

Parallel Agents를 사용한다면, `~/.cursor/worktrees/<project-name>`를
`worktree_dir`에 추가하세요:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.cursor/worktrees/my-app"]
```

Cursor는 각 에이전트 worktree를 이 프로젝트별 디렉터리 아래에 저장합니다. Coasts는
런타임에 `~`를 확장하고 이 경로를 외부 경로로 처리하므로, 바인드 마운트가 적용되려면
기존 인스턴스를 다시 생성해야 합니다:

```bash
coast rm my-instance
coast build
coast run my-instance
```

Coastfile 변경 후 worktree 목록은 즉시 업데이트되지만,
Cursor Parallel Agent worktree에 할당하려면 컨테이너 내부의 외부 바인드 마운트가 필요합니다.

## Coasts 가이드 배치 위치

### `AGENTS.md` 또는 `.cursor/rules/coast.md`

짧고 항상 활성화되는 Coast Runtime 규칙은 여기에 두세요:

- 가장 이식성 높은 프로젝트 지침을 원한다면 `AGENTS.md`를 사용하세요
- Cursor 네이티브 프로젝트 규칙과 설정 UI 지원을 원한다면 `.cursor/rules/coast.md`를 사용하세요
- 명확한 이유가 없다면 동일한 Coast Runtime 블록을 두 곳 모두에 중복하지 마세요

### `.cursor/skills/coasts/SKILL.md` 또는 공유 `.agents/skills/coasts/SKILL.md`

재사용 가능한 `/coasts` 워크플로는 여기에 두세요:

- Cursor 전용 저장소라면 `.cursor/skills/coasts/SKILL.md`가 자연스러운 위치입니다
- 여러 하네스를 사용하는 저장소라면, 정본 skill을
  `.agents/skills/coasts/SKILL.md`에 두세요; Cursor는 이를 직접 로드할 수 있습니다
- skill은 실제 `/coasts` 워크플로를 담당해야 합니다: `coast lookup`,
  `coast ls`, `coast run`, `coast assign`, `coast unassign`,
  `coast checkout`, 그리고 `coast ui`

### `.cursor/commands/coasts.md`

Cursor는 프로젝트 명령도 지원합니다. Coasts 문서 기준으로는, 명령은
선택 사항으로 취급하세요:

- 명시적인 `/coasts` 진입점을 원할 때만 명령을 추가하세요
- 한 가지 단순한 방법은 해당 명령이 같은 skill을 재사용하도록 하는 것입니다
- 명령에 별도의 자체 지침을 부여하면, 유지해야 할 워크플로 사본이
  두 개가 됩니다

### `.cursor/worktrees.json`

`.cursor/worktrees.json`은 Coasts 정책이 아니라 Cursor 자체의
worktree 부트스트랩에 사용하세요:

- 의존성 설치
- `.env` 파일 복사 또는 심볼릭 링크 생성
- 데이터베이스 마이그레이션 또는 기타 일회성 부트스트랩 단계 실행

Coast Runtime 규칙이나 Coast CLI 워크플로를
`.cursor/worktrees.json`으로 옮기지 마세요.

## 예시 레이아웃

### Cursor만 사용

```text
AGENTS.md
.cursor/skills/coasts/SKILL.md
.cursor/commands/coasts.md        # optional
.cursor/rules/coast.md            # optional alternative to AGENTS.md
.cursor/worktrees.json            # optional, for Parallel Agents bootstrap
```

### Cursor와 다른 하네스를 함께 사용

```text
AGENTS.md
CLAUDE.md
.agents/skills/coasts/SKILL.md
.agents/skills/coasts/agents/openai.yaml
.claude/skills/coasts -> ../../.agents/skills/coasts
.cursor/commands/coasts.md        # optional
```

## Coasts의 동작

- **실행** — `coast run <name>`은 최신 빌드에서 새 Coast 인스턴스를 생성합니다. `coast run <name> -w <worktree>`를 사용하면 Cursor worktree를 한 단계에서 생성하고 할당할 수 있습니다. [Run](../concepts_and_terminology/RUN.md)을 참조하세요.
- **현재 체크아웃** — Cursor가 사용자가 연 저장소에서 직접 작업 중인 경우,
  특별한 Cursor 처리가 필요하지 않습니다.
- **바인드 마운트** — Parallel Agents의 경우, Coasts는
  `~/.cursor/worktrees/<project-name>`를 컨테이너 내부의
  `/host-external-wt/{index}`에 마운트합니다.
- **탐색** — `git worktree list --porcelain`는 여전히 저장소 범위로 동작하므로, Coasts는
  현재 프로젝트에 속한 Cursor worktree만 표시합니다.
- **이름 지정** — Cursor Parallel Agent worktree는 Coasts의 CLI와 UI에서
  브랜치 이름으로 표시됩니다.
- **할당** — `coast assign`은 Cursor worktree가 선택되면 외부 바인드
  마운트 경로에서 `/workspace`를 다시 마운트합니다.
- **Gitignored 동기화** — 절대 경로를 사용해 호스트 파일시스템에서 계속 동작합니다.
- **고아 감지** — Cursor가 오래된 worktree를 정리하면, Coasts는 누락된 gitdir를 감지하고 필요 시 할당 해제를 할 수 있습니다.

## 예시

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.cursor/worktrees/my-app"]
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
- `~/.codex/worktrees/` — Codex worktree
- `~/.cursor/worktrees/my-app/` — Cursor Parallel Agent worktree

## 제한 사항

- Cursor Parallel Agents를 사용하지 않는다면, 단지 Cursor에서 편집 중이라는 이유만으로
  `~/.cursor/worktrees/<project-name>`를 추가하지 마세요.
- Coast Runtime 규칙은 항상 활성화되는 한 곳에만 두세요: `AGENTS.md` 또는
  `.cursor/rules/coast.md`. 둘 다 중복하면 내용이 어긋나기 쉽습니다.
- 재사용 가능한 `/coasts` 워크플로는 skill에 두세요. `.cursor/worktrees.json`은
  Cursor 부트스트랩용이지 Coasts 정책용이 아닙니다.
- 하나의 저장소를 Cursor, Codex, Claude Code, 또는 T3 Code에서 함께 사용한다면,
  [Multiple Harnesses](MULTIPLE_HARNESSES.md)의 공유 레이아웃을 우선하세요.
