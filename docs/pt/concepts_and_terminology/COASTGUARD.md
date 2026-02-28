# Coastguard

Coastguard é a UI web local do Coast (pense: uma interface no estilo Docker Desktop do Coast), executando na porta `31415`. Ela é iniciada a partir da CLI:

```bash
coast ui
```

![Coastguard project overview](../../assets/coastguard-overview.png)
*O painel do projeto mostrando instâncias do Coast em execução, suas branches/worktrees e o estado do checkout.*

![Coastguard port mappings](../../assets/coastguard-ports.png)
*A página de portas para uma instância específica do Coast, mostrando mapeamentos de portas canônicos e dinâmicos para cada serviço.*

## Para que o Coastguard é Bom

O Coastguard oferece uma superfície visual de controle e observabilidade para o seu projeto:

- Ver projetos, instâncias, status, branches e estado do checkout.
- Inspecionar [mapeamentos de portas](PORTS.md) e ir diretamente para os serviços.
- Visualizar [logs](LOGS.md), estatísticas de runtime e inspecionar dados.
- Navegar por [builds](BUILDS.md), artefatos de imagem, metadados de [volumes](VOLUMES.md) e [secrets](SECRETS.md).
- Navegar pela documentação no app enquanto trabalha.

## Relação com a CLI e o Daemon

O Coastguard não substitui a CLI. Ele a complementa como a interface voltada para humanos.

- A [CLI `coast`](CLI.md) é a interface de automação para scripts, fluxos de trabalho de agentes e integrações de ferramentas.
- O Coastguard é a interface humana para inspeção visual, depuração interativa e visibilidade operacional do dia a dia.
- Ambos são clientes do [`coastd`](DAEMON.md), então permanecem sincronizados.
