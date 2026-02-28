# Coast CLI

O Coast CLI (`coast`) é a interface principal de linha de comando para operar Coasts. Ele é intencionalmente enxuto: ele interpreta seu comando, envia uma solicitação para [`coastd`](DAEMON.md) e imprime saída estruturada de volta no seu terminal.

## Para Que Você Usa

Fluxos de trabalho típicos são todos conduzidos a partir da CLI:

```bash
coast build                                    # see Builds
coast run dev-1                                # see Coasts
coast assign dev-1 --worktree feature/oauth    # see Assign
coast ports dev-1                              # see Ports
coast checkout dev-1                           # see Checkout
coast ui                                       # see Coastguard
```

A CLI também inclui comandos de documentação que são úteis para humanos e agentes:

```bash
coast docs
coast docs --path concepts_and_terminology/CHECKOUT.md
coast search-docs "canonical vs dynamic ports"
```

## Por Que Ela Existe Separadamente do Daemon

Separar a CLI do daemon oferece alguns benefícios importantes:

- O daemon mantém estado e processos de longa duração.
- A CLI continua rápida, componível e fácil de automatizar via scripts.
- Você pode executar comandos pontuais sem manter o estado do terminal ativo.
- Ferramentas de agentes podem chamar comandos da CLI de maneiras previsíveis e amigáveis à automação.

## CLI vs Coastguard

Use a interface que fizer mais sentido no momento:

- A CLI é projetada para cobertura operacional completa: qualquer coisa que você possa fazer no Coastguard também deve ser possível a partir da CLI.
- Trate a CLI como a interface de automação — scripts, fluxos de trabalho de agentes, jobs de CI e ferramentas personalizadas de desenvolvedor.
- Trate o [Coastguard](COASTGUARD.md) como a interface humana — inspeção visual, depuração interativa e visibilidade operacional.

Ambos falam com o mesmo daemon, então operam sobre o mesmo estado subjacente do projeto.
