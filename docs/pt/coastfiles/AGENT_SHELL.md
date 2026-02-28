# Shell do Agente

> **Na maioria dos fluxos de trabalho, você não precisa containerizar seu agente de codificação.** Como as Coasts compartilham o [sistema de arquivos](../concepts_and_terminology/FILESYSTEM.md) com a sua máquina host, a abordagem mais simples é executar o agente no host e usar [`coast exec`](../concepts_and_terminology/EXEC_AND_DOCKER.md) para tarefas pesadas em tempo de execução, como testes de integração. Shells do agente são para casos em que você especificamente quer o agente rodando dentro do container — por exemplo, para dar a ele acesso direto ao daemon Docker interno ou para isolar completamente o ambiente.

A seção `[agent_shell]` configura um TUI de agente — como Claude Code ou Codex — para rodar dentro do container do Coast. Quando presente, o Coast automaticamente inicia uma sessão PTY persistente executando o comando configurado quando uma instância inicia.

Para uma visão completa de como shells do agente funcionam — o modelo de agente ativo, envio de entrada, ciclo de vida e recuperação — veja [Shells do Agente](../concepts_and_terminology/AGENT_SHELLS.md).

## Configuração

A seção tem um único campo obrigatório: `command`.

```toml
[agent_shell]
command = "claude --dangerously-skip-permissions"
```

### `command` (obrigatório)

O comando de shell a executar no PTY do agente. Isso normalmente é uma CLI de agente de codificação que você instalou via `[coast.setup]`.

O comando é executado dentro do container DinD em `/workspace` (a raiz do projeto). Ele não é um serviço do compose — ele roda ao lado da sua stack do compose ou serviços bare, não dentro deles.

## Ciclo de vida

- A shell do agente é iniciada automaticamente no `coast run`.
- No [Coastguard](../concepts_and_terminology/COASTGUARD.md), ela aparece como uma aba persistente "Agent" que não pode ser fechada.
- Se o processo do agente encerrar, o Coast pode iniciá-lo novamente.
- Você pode enviar entrada para uma shell do agente em execução via `coast agent-shell input`.

## Exemplos

### Claude Code

Instale o Claude Code em `[coast.setup]`, configure credenciais via [secrets](SECRETS.md), depois configure a shell do agente:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "git", "bash"]
run = [
    "npm install -g @anthropic-ai/claude-code",
    "mkdir -p /root/.claude",
]

[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[agent_shell]
command = "cd /workspace; exec claude --dangerously-skip-permissions --effort high"
```

### Shell do agente simples

Uma shell do agente mínima para testar se o recurso funciona:

```toml
[coast]
name = "test-agent"

[coast.setup]
packages = ["bash"]

[agent_shell]
command = "exec sh -c 'while true; do echo agent-heartbeat; sleep 5; done'"
```
