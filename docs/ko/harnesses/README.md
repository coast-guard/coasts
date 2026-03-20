# 하니스

각 하니스는 서로 다른 위치에 git 워크트리를 생성합니다. Coasts에서는
[`worktree_dir`](../coastfiles/WORKTREE_DIR.md) 배열이 어디를 찾아야 하는지 알려주며 --
추가 bind mount가 필요한 `~/.codex/worktrees` 같은 외부 경로도 포함됩니다.

각 하니스는 프로젝트 수준 지침, 스킬, 명령에 대해서도 자체적인 규칙을 가집니다. 아래 매트릭스는 각 하니스가 무엇을 지원하는지 보여주므로 Coasts에 대한 안내를 어디에 둘지 알 수 있습니다. 각 페이지는 Coastfile 구성, 권장 파일 레이아웃, 그리고 해당 하니스에 특화된 주의사항을 다룹니다.

하나의 저장소를 여러 하니스에서 사용하는 경우 [Multiple Harnesses](MULTIPLE_HARNESSES.md)를 참고하세요.

| Harness | Worktree location | Project instructions | Skills | Commands | Page |
|---------|-------------------|----------------------|--------|----------|------|
| OpenAI Codex | `~/.codex/worktrees` | `AGENTS.md` | `.agents/skills/` | Skills surface as `/` commands | [Codex](CODEX.md) |
| Claude Code | `.claude/worktrees` | `CLAUDE.md` | `.claude/skills/` | `.claude/commands/` | [Claude Code](CLAUDE_CODE.md) |
| Cursor | `~/.cursor/worktrees/<project>` | `AGENTS.md` or `.cursor/rules/` | `.cursor/skills/` or `.agents/skills/` | `.cursor/commands/` | [Cursor](CURSOR.md) |
| Conductor | `~/conductor/workspaces/<project>` | `CLAUDE.md` | -- | -- | [Conductor](CONDUCTOR.md) |
| T3 Code | `~/.t3/worktrees/<project>` | `AGENTS.md` | `.agents/skills/` | -- | [T3 Code](T3_CODE.md) |

## Skills vs Commands

스킬과 명령은 둘 다 재사용 가능한 `/coasts` 워크플로를 정의할 수 있게 해줍니다. 하니스가 무엇을 지원하는지에 따라 둘 중 하나만 쓰거나 둘 다 쓸 수 있습니다.

하니스가 명령을 지원하고 명시적인 `/coasts`
진입점을 원한다면, 간단한 방법 하나는 스킬을 재사용하는 명령을 추가하는 것입니다.
명령은 이름으로 명시적으로 호출되므로,
`/coasts` 워크플로가 언제 실행되는지 정확히 알 수 있습니다. 스킬도 에이전트가
문맥에 따라 자동으로 로드할 수 있는데, 이는 유용하지만 지침이
언제 불러와지는지에 대해서는 제어가 더 적다는 뜻이기도 합니다.

둘 다 사용할 수 있습니다. 그렇게 한다면,
워크플로의 별도 복사본을 유지하지 말고 명령이 스킬을 재사용하게 하세요.

하니스가 스킬만 지원한다면(T3 Code) 스킬을 사용하세요. 둘 다
지원하지 않는다면(Conductor) `/coasts` 워크플로를 프로젝트
지침 파일에 직접 넣으세요.
