# Receitas

Receitas são exemplos completos e anotados de Coastfile para formatos comuns de projetos. Cada receita inclui um Coastfile completo que você pode copiar e adaptar, seguido por um passo a passo seção por seção explicando por que cada decisão foi tomada.

Se você é novo em Coastfiles, comece primeiro com a [referência de Coastfiles](../coastfiles/README.md). As receitas pressupõem familiaridade com os conceitos centrais.

- [Monorepo Full-Stack](FULLSTACK_MONOREPO.md) - Postgres e Redis compartilhados no host, frontends Vite bare-service e um backend dockerizado via compose. Cobre estratégias de volume, healthchecks, ajuste de assign e `exclude_paths` para repositórios grandes.
- [Aplicação Next.js](NEXTJS.md) - Next.js com Turbopack, Postgres e Redis compartilhados, workers em segundo plano e tratamento dinâmico de portas para callbacks de autenticação. Cobre `private_paths` para isolamento de `.next`, otimização de bare service e suporte a worktrees com múltiplos agentes.
