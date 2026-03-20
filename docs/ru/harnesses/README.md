# Harnesses

Каждый harness создает git worktree в своем месте. В Coasts массив
[`worktree_dir`](../coastfiles/WORKTREE_DIR.md) указывает, где их искать --
включая внешние пути, такие как `~/.codex/worktrees`, для которых требуются
дополнительные bind mounts.

У каждого harness также есть свои соглашения для инструкций на уровне проекта, skills и commands. Матрица ниже показывает, что поддерживает каждый harness, чтобы вы знали, куда помещать указания для Coasts. На каждой странице описаны конфигурация Coastfile, рекомендуемая структура файлов и любые особенности, относящиеся именно к этому harness.

Если один репозиторий используется из нескольких harness, см. [Multiple Harnesses](MULTIPLE_HARNESSES.md).

| Harness | Worktree location | Project instructions | Skills | Commands | Page |
|---------|-------------------|----------------------|--------|----------|------|
| OpenAI Codex | `~/.codex/worktrees` | `AGENTS.md` | `.agents/skills/` | Skills surface as `/` commands | [Codex](CODEX.md) |
| Claude Code | `.claude/worktrees` | `CLAUDE.md` | `.claude/skills/` | `.claude/commands/` | [Claude Code](CLAUDE_CODE.md) |
| Cursor | `~/.cursor/worktrees/<project>` | `AGENTS.md` or `.cursor/rules/` | `.cursor/skills/` or `.agents/skills/` | `.cursor/commands/` | [Cursor](CURSOR.md) |
| Conductor | `~/conductor/workspaces/<project>` | `CLAUDE.md` | -- | -- | [Conductor](CONDUCTOR.md) |
| T3 Code | `~/.t3/worktrees/<project>` | `AGENTS.md` | `.agents/skills/` | -- | [T3 Code](T3_CODE.md) |

## Skills vs Commands

Skills и commands оба позволяют определить повторно используемый workflow `/coasts`. Вы можете использовать что-то одно или оба варианта, в зависимости от того, что поддерживает harness.

Если ваш harness поддерживает commands и вам нужна явная точка входа `/coasts`,
один из простых вариантов — добавить command, который переиспользует skill.
Commands явно вызываются по имени, поэтому вы точно знаете, когда запускается
workflow `/coasts`. Skills также могут автоматически загружаться агентом
на основе контекста, что полезно, но означает, что у вас меньше контроля над тем,
когда подтягиваются инструкции.

Вы можете использовать оба варианта. Если так и делаете, пусть command
переиспользует skill вместо того, чтобы поддерживать отдельную копию workflow.

Если harness поддерживает только skills (T3 Code), используйте skill. Если он не поддерживает
ни то ни другое (Conductor), поместите workflow `/coasts` прямо в файл
инструкций проекта.
