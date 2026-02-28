# Lookup

`coast lookup`은 호출자의 현재 작업 디렉터리에 대해 어떤 Coast 인스턴스가 실행 중인지 찾아냅니다. 이는 호스트 측 에이전트가 자신을 정렬하기 위해 가장 먼저 실행해야 하는 명령입니다 — "여기서 코드를 편집하고 있는데, 어떤 Coast(들)와 상호작용해야 하지?"

```bash
coast lookup
```

Lookup은 당신이 [worktree](ASSIGN.md) 내부에 있는지 또는 프로젝트 루트에 있는지 감지하고, 일치하는 인스턴스를 데몬에 질의한 뒤, 포트, URL, 그리고 예시 명령과 함께 결과를 출력합니다.

## Why This Exists

호스트에서 실행되는 AI 코딩 에이전트(Cursor, Claude Code, Codex 등)는 [shared filesystem](FILESYSTEM.md)을 통해 파일을 편집하고 런타임 작업을 위해 Coast CLI 명령을 호출합니다. 하지만 에이전트는 먼저 기본적인 질문에 답해야 합니다: **내가 작업 중인 디렉터리에 해당하는 Coast 인스턴스는 어느 것인가?**

`coast lookup`이 없다면, 에이전트는 `coast ls`를 실행하고, 전체 인스턴스 테이블을 파싱한 다음, 어떤 worktree에 있는지 알아내고, 상호 참조해야 합니다. `coast lookup`은 이 모든 작업을 한 단계로 수행하고 에이전트가 바로 소비할 수 있는 구조화된 출력을 반환합니다.

이 명령은 Coast를 사용하는 에이전트 워크플로를 위한 어떤 최상위 SKILL.md, AGENTS.md 또는 규칙 파일에도 포함되어야 합니다. 이는 에이전트가 자신의 런타임 컨텍스트를 발견하기 위한 진입점입니다.

## Output Modes

### Default (human-readable)

```bash
coast lookup
```

```text
Coast instances for worktree feature/oauth (my-app):

  dev-1  running  ★ checked out

  Primary URL:  http://dev-1.localhost:62217

  SERVICE              CANONICAL       DYNAMIC
  ★ web                3000            62217
    api                8080            63889
    postgres           5432            55681

  Examples (exec starts at the workspace root where your Coastfile is, cd to your target directory first):
    coast exec dev-1 -- sh -c "cd <dir> && <command>"
    coast logs dev-1 --service <service>
    coast ps dev-1
```

예시 섹션은 `coast exec`가 워크스페이스 루트 — 즉 Coastfile이 있는 디렉터리 — 에서 시작한다는 점을 에이전트(와 사람)에게 상기시킵니다. 하위 디렉터리에서 명령을 실행하려면 exec 내부에서 해당 디렉터리로 `cd` 하세요.

### Compact (`--compact`)

인스턴스 이름의 JSON 배열을 반환합니다. 어떤 인스턴스를 대상으로 삼아야 하는지만 알면 되는 스크립트 및 에이전트 도구를 위해 설계되었습니다.

```bash
coast lookup --compact
```

```text
["dev-1"]
```

동일한 worktree에 여러 인스턴스가 있는 경우:

```text
["dev-1","dev-2"]
```

일치 항목이 없는 경우:

```text
[]
```

### JSON (`--json`)

전체 구조화된 응답을 보기 좋게 출력된 JSON으로 반환합니다. 포트, URL, 상태를 기계 판독 가능한 형식으로 필요로 하는 에이전트를 위해 설계되었습니다.

```bash
coast lookup --json
```

```json
{
  "project": "my-app",
  "worktree": "feature/oauth",
  "project_root": "/Users/dev/my-app",
  "instances": [
    {
      "name": "dev-1",
      "status": "Running",
      "checked_out": true,
      "branch": "feature/oauth",
      "primary_url": "http://dev-1.localhost:62217",
      "ports": [
        { "logical_name": "web", "canonical_port": 3000, "dynamic_port": 62217, "is_primary": true },
        { "logical_name": "api", "canonical_port": 8080, "dynamic_port": 63889, "is_primary": false }
      ]
    }
  ]
}
```

## How It Resolves

Lookup은 현재 작업 디렉터리에서 위로 올라가며 가장 가까운 Coastfile을 찾은 다음, 어떤 worktree에 있는지 판단합니다:

1. cwd가 `{project_root}/{worktree_dir}/{name}/...` 아래에 있다면, lookup은 해당 worktree에 할당된 인스턴스를 찾습니다.
2. cwd가 프로젝트 루트(또는 worktree 내부가 아닌 어떤 디렉터리)라면, lookup은 **worktree가 할당되지 않은** — 여전히 프로젝트 루트를 가리키는 — 인스턴스를 찾습니다.

즉 lookup은 하위 디렉터리에서도 동작합니다. `my-app/.coasts/feature-oauth/src/api/`에 있더라도, lookup은 여전히 `feature-oauth`를 worktree로 해석합니다.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | 하나 이상의 일치하는 인스턴스를 찾음 |
| 1 | 일치하는 인스턴스가 없음(빈 결과) |

이로 인해 lookup은 셸 조건문에서 사용할 수 있습니다:

```bash
if coast lookup > /dev/null 2>&1; then
  coast exec dev-1 -- sh -c "cd src && npm test"
fi
```

## For Agent Workflows

전형적인 에이전트 통합 패턴:

1. 에이전트가 worktree 디렉터리에서 작업을 시작합니다.
2. 에이전트가 `coast lookup`을 실행하여 인스턴스 이름, 포트, URL, 예시 명령을 발견합니다.
3. 에이전트가 이후의 모든 Coast 명령에서 인스턴스 이름을 사용합니다: `coast exec`, `coast logs`, `coast ps`.

```text
┌─── Agent (host machine) ────────────────────────────┐
│                                                      │
│  1. coast lookup                                     │
│       → instance names, ports, URLs, examples        │
│  2. coast exec dev-1 -- sh -c "cd src && npm test"   │
│  3. coast logs dev-1 --service web --tail 50         │
│  4. coast ps dev-1                                   │
│                                                      │
└──────────────────────────────────────────────────────┘
```

에이전트가 여러 worktree에 걸쳐 작업 중이라면, 각 worktree 디렉터리에서 `coast lookup`을 실행하여 각 컨텍스트에 대한 올바른 인스턴스를 해석합니다.

호스트 에이전트가 Coast와 상호작용하는 방법은 [Filesystem](FILESYSTEM.md)을, worktree 개념은 [Assign and Unassign](ASSIGN.md)을, Coast 내부에서 명령을 실행하는 방법은 [Exec & Docker](EXEC_AND_DOCKER.md)을 참고하세요.
