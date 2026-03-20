# Harnesses

Cada harness cria git worktrees em um local diferente. No Coasts, o array
[`worktree_dir`](../coastfiles/WORKTREE_DIR.md) informa onde ele deve procurar --
incluindo caminhos externos como `~/.codex/worktrees` que exigem bind mounts
adicionais.

Cada harness também tem suas próprias convenções para instruções no nível do projeto, skills e comandos. A matriz abaixo mostra o que cada harness oferece para que você saiba onde colocar orientações para o Coasts. Cada página cobre a configuração do Coastfile, o layout de arquivos recomendado e quaisquer ressalvas específicas desse harness.

Se um repositório for usado por vários harnesses, consulte [Multiple Harnesses](MULTIPLE_HARNESSES.md).

| Harness | Worktree location | Project instructions | Skills | Commands | Page |
|---------|-------------------|----------------------|--------|----------|------|
| OpenAI Codex | `~/.codex/worktrees` | `AGENTS.md` | `.agents/skills/` | Skills aparecem como comandos `/` | [Codex](CODEX.md) |
| Claude Code | `.claude/worktrees` | `CLAUDE.md` | `.claude/skills/` | `.claude/commands/` | [Claude Code](CLAUDE_CODE.md) |
| Cursor | `~/.cursor/worktrees/<project>` | `AGENTS.md` ou `.cursor/rules/` | `.cursor/skills/` ou `.agents/skills/` | `.cursor/commands/` | [Cursor](CURSOR.md) |
| Conductor | `~/conductor/workspaces/<project>` | `CLAUDE.md` | -- | -- | [Conductor](CONDUCTOR.md) |
| T3 Code | `~/.t3/worktrees/<project>` | `AGENTS.md` | `.agents/skills/` | -- | [T3 Code](T3_CODE.md) |

## Skills vs Commands

Skills e comandos permitem definir um fluxo de trabalho `/coasts` reutilizável. Você pode usar um ou ambos, dependendo do que o harness suporta.

Se o seu harness suporta comandos e você quer um ponto de entrada `/coasts`
explícito, uma opção simples é adicionar um comando que reutilize a skill.
Comandos são invocados explicitamente pelo nome, então você sabe exatamente
quando o fluxo de trabalho `/coasts` é executado. Skills também podem ser
carregadas automaticamente pelo agente com base no contexto, o que é útil, mas
significa que você tem menos controle sobre quando as instruções são incluídas.

Você pode usar ambos. Se fizer isso, deixe o comando reutilizar a skill em vez
de manter uma cópia separada do fluxo de trabalho.

Se o harness suporta apenas skills (T3 Code), use uma skill. Se não suporta
nenhum dos dois (Conductor), coloque o fluxo de trabalho `/coasts` diretamente
no arquivo de instruções do projeto.
