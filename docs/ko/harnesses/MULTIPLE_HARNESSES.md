# 여러 하네스

하나의 저장소가 둘 이상의 하네스에서 사용되는 경우, Coasts 설정을 통합하는 한 가지 방법은
공유되는 `/coasts` 워크플로를 한 곳에 유지하고 하네스별 상시 적용 규칙은 각 하네스의 파일에
유지하는 것입니다.

## 권장 레이아웃

```text
AGENTS.md
CLAUDE.md
.cursor/rules/coast.md           # optional Cursor-native always-on rules
.agents/skills/coasts/SKILL.md
.agents/skills/coasts/agents/openai.yaml
.claude/skills/coasts -> ../../.agents/skills/coasts
.cursor/commands/coasts.md       # optional, thin, harness-specific
.claude/commands/coasts.md   # optional, thin, harness-specific
```

이 레이아웃은 다음과 같이 사용하세요:

- `AGENTS.md` — Codex 및 T3 Code에서 Coasts로 작업하기 위한 짧은 상시 적용 규칙
- `.cursor/rules/coast.md` — 선택적인 Cursor 기본 상시 적용 규칙
- `CLAUDE.md` — Claude Code 및 Conductor에서 Coasts로 작업하기 위한 짧은 상시 적용 규칙
- `.agents/skills/coasts/SKILL.md` — 표준 재사용 가능 `/coasts` 워크플로
- `.agents/skills/coasts/agents/openai.yaml` — 선택적인 Codex/OpenAI 메타데이터
- `.claude/skills/coasts` — Claude Code에서도 같은 스킬이 필요할 때 사용하는 Claude용 미러 또는 심볼릭 링크
- `.cursor/commands/coasts.md` — 선택적인 Cursor 명령 파일; 간단한 한 가지 방법은 같은 스킬을 재사용하게 하는 것입니다
- `.claude/commands/coasts.md` — 선택적인 명시적 명령 파일; 간단한 한 가지 방법은 같은 스킬을 재사용하게 하는 것입니다

## 단계별 안내

1. Coast Runtime 규칙을 상시 적용 지시 파일에 넣습니다.
   - `AGENTS.md`, `CLAUDE.md`, 또는 `.cursor/rules/coast.md`는 "모든 작업" 규칙에 답해야 합니다: 먼저 `coast lookup` 실행, `coast exec` 사용, `coast logs`로 로그 읽기, 일치 항목이 없을 때 `coast assign` 또는 `coast run` 전에 확인 요청.
2. Coasts용 표준 스킬 하나를 만듭니다.
   - 재사용 가능한 `/coasts` 워크플로를 `.agents/skills/coasts/SKILL.md`에 넣습니다.
   - 해당 스킬 안에서 Coast CLI를 직접 사용합니다: `coast lookup`,
     `coast ls`, `coast run`, `coast assign`, `coast unassign`,
     `coast checkout`, 그리고 `coast ui`.
3. 하네스에 다른 경로가 필요한 곳에만 그 스킬을 노출합니다.
   - Codex, T3 Code, Cursor는 모두 `.agents/skills/`를 직접 사용할 수 있습니다.
   - Claude Code는 `.claude/skills/`가 필요하므로, 표준 스킬을 그 위치에 미러링하거나 심볼릭 링크로 연결합니다.
4. 명시적인 `/coasts` 진입점을 원할 때만 명령 파일을 추가합니다.
   - `.claude/commands/coasts.md` 또는
     `.cursor/commands/coasts.md`를 만든다면, 간단한 한 가지 방법은 명령이 같은 스킬을 재사용하게 하는 것입니다.
   - 명령에 자체적인 별도 지침을 부여하면, 유지 관리해야 할 워크플로의 두 번째 복사본을 떠안게 됩니다.
5. Conductor 전용 설정은 스킬이 아니라 Conductor에 유지합니다.
   - Conductor 자체에 속하는 bootstrap 또는 실행 동작에는 Conductor Repository Settings 스크립트를 사용합니다.
   - Coasts 정책과 `coast` CLI 사용은 `CLAUDE.md`와 공유 스킬에 유지합니다.

## 구체적인 `/coasts` 예시

좋은 공유 `coasts` 스킬은 세 가지 작업을 수행해야 합니다:

1. `Use Existing Coast`
   - `coast lookup` 실행
   - 일치 항목이 있으면 `coast exec`, `coast ps`, `coast logs` 사용
2. `Manage Assignment`
   - `coast ls` 실행
   - `coast run`, `coast assign`, `coast unassign`, 또는
     `coast checkout` 제안
   - 기존 슬롯을 재사용하거나 방해하기 전에 확인 요청
3. `Open UI`
   - `coast ui` 실행

이것이 `/coasts` 워크플로를 두기에 적절한 위치입니다. 상시 적용 파일에는
스킬이 전혀 호출되지 않더라도 반드시 적용되어야 하는 짧은 규칙만 담아야 합니다.

## 심볼릭 링크 패턴

Claude Code가 Codex, T3 Code, 또는 Cursor와 같은 스킬을 재사용하게 하려면,
한 가지 방법은 심볼릭 링크를 사용하는 것입니다:

```bash
mkdir -p .claude/skills
ln -s ../../.agents/skills/coasts .claude/skills/coasts
```

팀이 심볼릭 링크 사용을 선호하지 않는다면 저장소에 포함된 미러도 괜찮습니다. 주요 목표는
복사본 간의 불필요한 차이를 피하는 것입니다.

## 하네스별 주의사항

- Claude Code: 프로젝트 스킬과 선택적인 프로젝트 명령은 모두 유효하지만,
  로직은 스킬에 유지하세요.
- Cursor: 짧은 Coast Runtime 규칙에는 `AGENTS.md` 또는 `.cursor/rules/coast.md`를 사용하고, 재사용 가능한 워크플로에는 스킬을 사용하며,
  `.cursor/commands`는 선택 사항으로 유지하세요.
- Conductor: 우선 `CLAUDE.md`와 Conductor 스크립트 및 설정의 조합으로 취급하세요.
  명령을 추가했는데 표시되지 않으면, 다시 확인하기 전에 앱을 완전히 종료했다가 다시 여세요.
- T3 Code: 여기서는 가장 얇은 하네스 표면입니다. Codex 스타일의
  `AGENTS.md`와 `.agents/skills` 패턴을 사용하고, Coasts 문서를 위해 별도의
  T3 전용 명령 레이아웃을 새로 만들지 마세요.
- Codex: `AGENTS.md`는 짧게 유지하고 재사용 가능한 워크플로는
  `.agents/skills`에 두세요.
