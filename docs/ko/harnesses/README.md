# 하니스

대부분의 하니스는 작업을 병렬로 실행하기 위해 git 워크트리를 생성합니다. 이러한 워크트리는 프로젝트 내부에 있을 수도 있고 완전히 외부에 있을 수도 있습니다. Coast의 [`worktree_dir`](../coastfiles/WORKTREE_DIR.md) 배열은 추가 bind mount가 필요한 `~/.codex/worktrees` 같은 외부 경로를 포함하여 어디를 살펴볼지 알려줍니다.

아래의 각 페이지는 해당 하니스에 특화된 Coastfile 구성과 주의사항을 다룹니다.

| Harness | Worktree location | Page |
|---------|-------------------|------|
| Conductor | `~/conductor/workspaces/<project>` | [Conductor](CONDUCTOR.md) |
| OpenAI Codex | `~/.codex/worktrees` | [Codex](CODEX.md) |
| T3 Code | `~/.t3/worktrees/<project>` | [T3 Code](T3_CODE.md) |
