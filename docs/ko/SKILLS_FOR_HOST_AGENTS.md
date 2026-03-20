# 호스트 에이전트를 위한 스킬

앱이 Coasts 내부에서 실행되는 동안 호스트에서 AI 코딩 에이전트를 사용한다면,
에이전트에는 보통 Coast 전용 설정이 두 가지 필요합니다:

1. 하네스의 프로젝트 지침 파일 또는 규칙 파일에 항상 적용되는 Coast Runtime 섹션
2. 하네스가 프로젝트 스킬을 지원할 때 `/coasts` 같은 재사용 가능한 Coast 워크플로 스킬

첫 번째가 없으면 에이전트는 파일을 편집하지만 `coast exec` 사용을 잊습니다.
두 번째가 없으면 모든 Coast 할당, 로그, UI 흐름을 채팅에서 매번 다시
설명해야 합니다.

이 가이드는 설정을 구체적이고 Coast에 맞게 유지합니다. 어떤 파일을 만들고,
그 안에 어떤 텍스트를 넣어야 하는지, 그리고 그것이 하네스에 따라 어떻게
달라지는지를 설명합니다.

## 에이전트에 이것이 필요한 이유

Coasts는 호스트 머신과 Coast 컨테이너 사이에서
[filesystem](concepts_and_terminology/FILESYSTEM.md)을 공유합니다. 에이전트는 호스트에서 파일을 편집하고,
Coast 내부에서 실행 중인 서비스는 그 변경 사항을 즉시 확인합니다. 하지만
에이전트는 여전히 다음을 해야 합니다:

1. 현재 체크아웃에 맞는 Coast 인스턴스를 찾기
2. 해당 Coast 내부에서 테스트, 빌드, 런타임 명령 실행하기
3. Coast에서 로그와 서비스 상태 읽기
4. 아직 연결된 Coast가 없을 때 worktree 할당을 안전하게 처리하기

## 무엇을 어디에 두어야 하나

- `AGENTS.md`, `CLAUDE.md`, 또는 `.cursor/rules/coast.md` — 어떤 스킬도
  호출되지 않았더라도 모든 작업에 적용되어야 하는 짧은 Coast 규칙
- 스킬 (`.agents/skills/...`, `.claude/skills/...`, 또는 `.cursor/skills/...`)
  — `/coasts` 같은 재사용 가능한 Coast 워크플로 자체
- 명령 파일 (`.claude/commands/...` 또는 `.cursor/commands/...`) — 선택 사항인
  명시적 진입점으로, 이를 지원하는 하네스에서 사용할 수 있음; 한 가지 단순한
  방법은 명령이 스킬을 재사용하게 하는 것

하나의 저장소가 둘 이상의 하네스를 사용한다면, 정본 Coast 스킬은 한 곳에
유지하고 필요한 곳에 노출하세요. 자세한 내용은
[Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md)를 참고하세요.

## 1. 항상 적용되는 Coast Runtime 규칙

다음 블록을 하네스의 항상 적용되는 프로젝트 지침 파일 또는 규칙 파일
(`AGENTS.md`, `CLAUDE.md`, `.cursor/rules/coast.md` 또는 이에 준하는 파일)에
추가하세요:

```text-copy
# Coast Runtime

This project uses Coasts — containerized runtimes for running services, tests,
and other runtime commands. The filesystem is shared between the host and the
container, so file edits on either side are visible to both immediately.

## Discovery

Before the first runtime command in a session, run:

  coast lookup

This prints the instance name, ports, and example commands. Use the instance
name from the output for all subsequent commands.

## What runs where

The filesystem is shared, so only use `coast exec` for things that need the
container runtime (databases, services, integration tests). Everything else
runs directly on the host.

Use `coast exec` for:
- Tests that need running services (integration tests, API tests)
- Service restarts or compose operations
- Anything that talks to databases, caches, or other container services

Run directly on the host:
- Linting, typechecking, formatting
- Git operations
- Playwright and browser tests
- Installing host-side dependencies (npm install, pip install)
- File search, code generation, static analysis

Example:

  coast exec <instance> -- sh -c "cd <dir> && npm test"    # needs DB
  npm run lint                                              # host is fine
  npx playwright test                                       # host is fine

## Runtime feedback

  coast ps <instance>
  coast logs <instance> --service <service>
  coast logs <instance> --service <service> --tail 50

## Creating and assigning Coasts

If `coast lookup` returns no match, run `coast ls` to see what exists.

If an unassigned Coast is already running for this project, prefer assigning
your worktree to it rather than creating a new one:

  coast assign <existing> -w <worktree>

If no Coast is running, ask the user before creating one — Coasts can be
memory intensive:

  coast run <name> -w <worktree>

A project must be built before instances can be created. If `coast run` fails
because no build exists, run `coast build` first.

## Coastfile setup

If the project does not have a Coastfile yet, or if you need to modify the
Coastfile, read the Coastfile docs first:

  coast docs --path coastfiles/README.md

## When confused

Before guessing about Coast behavior, explore the docs:

  coast docs                                     # list all doc pages
  coast docs --path concepts_and_terminology/RUN.md
  coast docs --path concepts_and_terminology/ASSIGN.md
  coast docs --path concepts_and_terminology/BUILDS.md
  coast search-docs "your question here"         # semantic search

## Rules

- Always run `coast lookup` before your first runtime command in a session.
- Use `coast exec` only for things that need the container runtime.
- Run linting, typechecking, formatting, and git on the host directly.
- Use `coast docs` or `coast search-docs` before guessing about Coast behavior.
- Do not run services directly on the host when the project expects Coast.
```

이 블록은 항상 적용되는 파일에 들어가야 합니다. 규칙은 에이전트가 명시적으로
`/coasts` 워크플로에 들어갈 때만이 아니라 모든 작업에 적용되어야 하기
때문입니다.

## 2. 재사용 가능한 `/coasts` 스킬

하네스가 프로젝트 스킬을 지원한다면, 스킬 내용을 스킬 디렉터리의 `SKILL.md`로
저장하세요. 전체 스킬 텍스트는 [skills_prompt.txt](skills_prompt.txt)에
있습니다(CLI 모드라면 `coast skills-prompt` 사용) — Coast Runtime 블록 뒤의
모든 내용이 스킬 콘텐츠이며, `---` 프런트매터부터 시작합니다.

Codex 또는 OpenAI 전용 표면을 사용 중이라면, 표시 메타데이터나 호출 정책을
위해 스킬 옆에 `agents/openai.yaml`을 선택적으로 추가할 수 있습니다. 그
메타데이터는 스킬을 대체하는 것이 아니라 스킬 옆에 있어야 합니다.

## 하네스 빠른 시작

| Harness | Always-on file | Reusable Coast workflow | Notes |
|---------|----------------|-------------------------|-------|
| OpenAI Codex | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | Coast 문서를 위해 별도로 권장할 프로젝트 명령 파일은 없습니다. [Codex](harnesses/CODEX.md)를 참고하세요. |
| Claude Code | `CLAUDE.md` | `.claude/skills/coasts/SKILL.md` | `.claude/commands/coasts.md`는 선택 사항이지만, 로직은 스킬에 유지하세요. [Claude Code](harnesses/CLAUDE_CODE.md)를 참고하세요. |
| Cursor | `AGENTS.md` or `.cursor/rules/coast.md` | `.cursor/skills/coasts/SKILL.md` or shared `.agents/skills/coasts/SKILL.md` | `.cursor/commands/coasts.md`는 선택 사항입니다. `.cursor/worktrees.json`은 Cursor worktree 부트스트랩용이지 Coast 정책용이 아닙니다. [Cursor](harnesses/CURSOR.md)를 참고하세요. |
| Conductor | `CLAUDE.md` | `CLAUDE.md`로 시작하고, Conductor 전용 동작에는 Conductor 스크립트와 설정을 사용하세요 | Claude Code의 전체 프로젝트 명령 동작을 가정하지 마세요. 새 명령이 나타나지 않으면 Conductor를 완전히 종료했다가 다시 여세요. [Conductor](harnesses/CONDUCTOR.md)를 참고하세요. |
| T3 Code | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | 여기서는 가장 제한적인 하네스 표면입니다. Codex 스타일 레이아웃을 사용하고 Coast 문서를 위한 T3 전용 명령 계층을 새로 만들지 마세요. [T3 Code](harnesses/T3_CODE.md)를 참고하세요. |

## 에이전트가 스스로 설정하게 하기

가장 빠른 방법은 에이전트가 올바른 파일을 직접 쓰게 하는 것입니다. 아래
프롬프트를 에이전트의 채팅에 복사해 넣으세요 — 여기에는 Coast Runtime 블록,
`coasts` 스킬 블록, 그리고 각 조각이 어디에 들어가야 하는지에 대한 하네스별
지침이 포함되어 있습니다.

```prompt-copy
skills_prompt.txt
```

CLI에서 `coast skills-prompt`를 실행해도 동일한 출력을 얻을 수 있습니다.

## 수동 설정

- **Codex:** Coast Runtime 섹션을 `AGENTS.md`에 넣고, 그다음 재사용 가능한
  `coasts` 스킬을 `.agents/skills/coasts/SKILL.md`에 넣으세요.
- **Claude Code:** Coast Runtime 섹션을 `CLAUDE.md`에 넣고, 그다음 재사용
  가능한 `coasts` 스킬을 `.claude/skills/coasts/SKILL.md`에 넣으세요.
  명령 파일이 특별히 필요할 때만 `.claude/commands/coasts.md`를 추가하세요.
- **Cursor:** 가장 이식성 높은 지침을 원한다면 Coast Runtime 섹션을
  `AGENTS.md`에 넣고, Cursor 네이티브 프로젝트 규칙을 원한다면
  `.cursor/rules/coast.md`에 넣으세요. 재사용 가능한 `coasts` 워크플로는
  Cursor 전용 저장소라면 `.cursor/skills/coasts/SKILL.md`에, 다른 하네스와
  저장소를 공유한다면 `.agents/skills/coasts/SKILL.md`에 넣으세요. 명시적
  명령 파일이 특별히 필요할 때만 `.cursor/commands/coasts.md`를 추가하세요.
- **Conductor:** Coast Runtime 섹션을 `CLAUDE.md`에 넣으세요. Conductor 전용
  부트스트랩 또는 실행 동작에는 Conductor Repository Settings 스크립트를
  사용하세요. 명령을 추가했는데 나타나지 않으면 앱을 완전히 종료했다가 다시
  여세요.
- **T3 Code:** Codex와 동일한 레이아웃을 사용하세요: `AGENTS.md`와
  `.agents/skills/coasts/SKILL.md`. 여기서는 T3 Code를 별도의 Coast 명령
  표면이 아니라 얇은 Codex 스타일 하네스로 취급하세요.
- **여러 하네스:** 정본 스킬은 `.agents/skills/coasts/SKILL.md`에
  유지하세요. Cursor는 그것을 직접 로드할 수 있고, 필요하다면 Claude Code에는
  `.claude/skills/coasts/`를 통해 노출하세요.

## 추가 읽을거리

- 하네스별 매트릭스는 [Harnesses guide](harnesses/README.md)를 읽어보세요
- 공유 레이아웃 패턴은 [Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md)를
  읽어보세요
- 전체 구성 스키마를 배우려면 [Coastfiles documentation](coastfiles/README.md)을
  읽어보세요
- 인스턴스 관리를 위한 명령은 [Coast CLI](concepts_and_terminology/CLI.md)를
  익혀두세요
- Coasts를 관찰하고 제어하는 웹 UI인
  [Coastguard](concepts_and_terminology/COASTGUARD.md)를 살펴보세요
