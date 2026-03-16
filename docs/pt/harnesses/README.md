# Harnesses

A maioria dos harnesses cria worktrees do git para executar tarefas em paralelo. Esses worktrees podem ficar dentro do seu projeto ou totalmente fora dele. O array [`worktree_dir`](../coastfiles/WORKTREE_DIR.md) do Coast informa onde procurar -- incluindo caminhos externos como `~/.codex/worktrees` que exigem bind mounts adicionais.

Cada página abaixo cobre a configuração do Coastfile e quaisquer ressalvas específicas desse harness.

| Harness | Localização do worktree | Página |
|---------|-------------------|------|
| OpenAI Codex | `~/.codex/worktrees` | [Codex](CODEX.md) |
