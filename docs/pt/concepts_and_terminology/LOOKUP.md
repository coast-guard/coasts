# Lookup

`coast lookup` descobre quais instâncias do Coast estão em execução para o diretório de trabalho atual do chamador. É o primeiro comando que um agente do lado do host deve executar para se orientar — "Estou editando código aqui, com qual(is) Coast(s) devo interagir?"

```bash
coast lookup
```

O Lookup detecta se você está dentro de uma [worktree](ASSIGN.md) ou na raiz do projeto, consulta o daemon por instâncias correspondentes e imprime os resultados com portas, URLs e comandos de exemplo.

## Why This Exists

Um agente de codificação com IA executando no host (Cursor, Claude Code, Codex, etc.) edita arquivos por meio do [sistema de arquivos compartilhado](FILESYSTEM.md) e chama comandos do Coast CLI para operações em tempo de execução. Mas o agente primeiro precisa responder a uma pergunta básica: **qual instância do Coast corresponde ao diretório em que estou trabalhando?**

Sem `coast lookup`, o agente teria que executar `coast ls`, analisar a tabela completa de instâncias, descobrir em qual worktree está e fazer a correlação. `coast lookup` faz tudo isso em uma única etapa e retorna saída estruturada que os agentes podem consumir diretamente.

Este comando deve ser incluído em qualquer arquivo SKILL.md, AGENTS.md ou de regras de nível superior para fluxos de trabalho de agentes que usam Coast. É o ponto de entrada para um agente descobrir seu contexto de runtime.

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

A seção de exemplos lembra os agentes (e humanos) de que `coast exec` inicia na raiz do workspace — o diretório onde o Coastfile fica. Para executar um comando em um subdiretório, faça `cd` para ele dentro do exec.

### Compact (`--compact`)

Retorna um array JSON de nomes de instância. Projetado para scripts e ferramentas de agentes que só precisam saber quais instâncias devem ser alvo.

```bash
coast lookup --compact
```

```text
["dev-1"]
```

Múltiplas instâncias no mesmo worktree:

```text
["dev-1","dev-2"]
```

Nenhuma correspondência:

```text
[]
```

### JSON (`--json`)

Retorna a resposta estruturada completa como JSON com pretty-print. Projetado para agentes que precisam de portas, URLs e status em um formato legível por máquina.

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

O Lookup sobe a partir do diretório de trabalho atual para encontrar o Coastfile mais próximo e, em seguida, determina em qual worktree você está:

1. Se seu cwd estiver sob `{project_root}/{worktree_dir}/{name}/...`, o lookup encontra instâncias atribuídas a essa worktree.
2. Se seu cwd for a raiz do projeto (ou qualquer diretório que não esteja dentro de uma worktree), o lookup encontra instâncias **sem worktree atribuída** — aquelas ainda apontadas para a raiz do projeto.

Isso significa que o lookup funciona também a partir de subdiretórios. Se você estiver em `my-app/.coasts/feature-oauth/src/api/`, o lookup ainda resolve `feature-oauth` como a worktree.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Uma ou mais instâncias correspondentes encontradas |
| 1 | Nenhuma instância correspondente (resultado vazio) |

Isso torna o lookup utilizável em condicionais de shell:

```bash
if coast lookup > /dev/null 2>&1; then
  coast exec dev-1 -- sh -c "cd src && npm test"
fi
```

## For Agent Workflows

O padrão típico de integração de agente:

1. O agente começa a trabalhar em um diretório de worktree.
2. O agente executa `coast lookup` para descobrir nomes de instância, portas, URLs e comandos de exemplo.
3. O agente usa o nome da instância para todos os comandos subsequentes do Coast: `coast exec`, `coast logs`, `coast ps`.

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

Se o agente estiver trabalhando em múltiplas worktrees, ele executa `coast lookup` a partir de cada diretório de worktree para resolver a instância correta para cada contexto.

Veja também [Filesystem](FILESYSTEM.md) para como agentes no host interagem com o Coast, [Assign and Unassign](ASSIGN.md) para conceitos de worktree e [Exec & Docker](EXEC_AND_DOCKER.md) para executar comandos dentro de um Coast.
