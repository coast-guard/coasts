# Shells de Agente

Shells de agente são shells dentro de um Coast que abrem diretamente para um runtime TUI de agente — Claude Code, Codex, ou qualquer agente CLI. Você as configura com uma seção `[agent_shell]` no seu Coastfile e o Coast inicia o processo do agente dentro do contêiner DinD.

**Para a maioria dos casos de uso, você não deve fazer isso.** Em vez disso, execute seus agentes de codificação na máquina host. O [filesystem](FILESYSTEM.md) compartilhado significa que um agente do lado do host pode editar código normalmente enquanto chama [`coast logs`](LOGS.md), [`coast exec`](EXEC_AND_DOCKER.md) e [`coast ps`](RUNTIMES_AND_SERVICES.md) para obter informações de runtime. Shells de agente adicionam montagem de credenciais, complicações de OAuth e complexidade de ciclo de vida que você não precisa, a menos que tenha um motivo específico para containerizar o próprio agente.

## O Problema do OAuth

Se você estiver usando Claude Code, Codex ou ferramentas semelhantes que autenticam via OAuth, o token foi emitido para sua máquina host. Quando esse mesmo token é usado de dentro de um contêiner Linux — user agent diferente, ambiente diferente — o provedor pode sinalizá-lo ou revogá-lo. Você terá falhas de autenticação intermitentes que são difíceis de depurar.

Para agentes containerizados, a autenticação baseada em chave de API é a opção mais segura. Defina a chave como um [segredo](SECRETS.md) no seu Coastfile e injete-a no ambiente do contêiner.

Se chaves de API não forem uma opção, você pode montar credenciais OAuth no Coast (veja a seção Configuração abaixo), mas espere atrito. No macOS, se você usar o extrator de segredos `keychain` para obter tokens OAuth, cada `coast build` solicitará sua senha do Chaveiro do macOS. Isso torna o processo de build tedioso, especialmente ao reconstruir com frequência. O prompt do Chaveiro é um requisito de segurança do macOS e não pode ser ignorado.

## Configuração

Adicione uma seção `[agent_shell]` ao seu Coastfile com o comando a executar:

```toml
[agent_shell]
command = "claude --dangerously-skip-permissions"
```

O comando é executado dentro do contêiner DinD em `/workspace`. O Coast cria um usuário `coast` dentro do contêiner, copia credenciais de `/root/.claude/` para `/home/coast/.claude/` e executa o comando como esse usuário. Se o seu agente precisa de credenciais montadas no contêiner, use `[secrets]` com injeção de arquivo (veja [Segredos e Extratores](SECRETS.md)) e `[coast.setup]` para instalar a CLI do agente:

```toml
[coast.setup]
run = ["npm install -g @anthropic-ai/claude-code"]

[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[agent_shell]
command = "claude --dangerously-skip-permissions"
```

Se `[agent_shell]` estiver configurado, o Coast inicia automaticamente uma shell quando a instância inicia. A configuração é herdada via `extends` e pode ser sobrescrita por [tipo de Coastfile](COASTFILE_TYPES.md).

## O Modelo de Agente Ativo

Cada instância do Coast pode ter múltiplas shells de agente, mas apenas uma é **ativa** por vez. A shell ativa é o alvo padrão para comandos que não especificam um ID `--shell`.

```bash
coast agent-shell dev-1 ls

  SHELL  STATUS   ACTIVE
  1      running  ★
  2      running
```

Troque a shell ativa:

```bash
coast agent-shell dev-1 activate 2
```

Você não pode fechar a shell ativa — primeiro ative uma diferente. Isso evita matar acidentalmente a shell com a qual você está interagindo.

No Coastguard, shells de agente aparecem como abas no painel Exec com badges de ativo/inativo. Clique em uma aba para ver seu terminal; use o menu suspenso para ativar, iniciar (spawn) ou fechar shells.

![Agent shell in Coastguard](../../assets/coastguard-agent-shell.png)
*Uma shell de agente executando Claude Code dentro de uma instância do Coast, acessível pela aba Exec no Coastguard.*

## Enviando Entrada

A principal forma de controlar um agente containerizado programaticamente é `coast agent-shell input`:

```bash
coast agent-shell dev-1 input "fix the failing test in auth.test.ts"
```

Isso escreve o texto na TUI do agente ativo e pressiona Enter. O agente o recebe como se você o tivesse digitado no terminal.

Opções:

- `--no-send` — escreve o texto sem pressionar Enter. Útil para construir entrada parcial ou navegar por menus de TUI.
- `--shell <id>` — direciona para uma shell específica em vez da ativa.
- `--show-bytes` — imprime os bytes exatos sendo enviados, para depuração.

Por baixo dos panos, a entrada é escrita diretamente no descritor de arquivo mestre do PTY. O texto e a tecla Enter são enviados como duas escritas separadas com um intervalo de 25ms para evitar artefatos de modo de colagem que alguns frameworks TUI exibem ao receber entrada rápida.

## Outros Comandos

```bash
coast agent-shell dev-1 spawn              # criar uma nova shell
coast agent-shell dev-1 spawn --activate   # criar e ativar imediatamente
coast agent-shell dev-1 tty                # anexar TTY interativa à shell ativa
coast agent-shell dev-1 tty --shell 2      # anexar a uma shell específica
coast agent-shell dev-1 read-output        # ler o buffer completo de scrollback
coast agent-shell dev-1 read-last-lines 50 # ler as últimas 50 linhas de saída
coast agent-shell dev-1 session-status     # verificar se o processo da shell está vivo
```

`tty` oferece uma sessão interativa ao vivo — você pode digitar diretamente na TUI do agente. Desanexe com a sequência de escape padrão do terminal. `read-output` e `read-last-lines` são não interativos e retornam texto, o que é útil para scripts e automação.

## Ciclo de Vida e Recuperação

Sessões de shell de agente persistem no Coastguard ao navegar entre páginas. O buffer de scrollback (até 512KB) é reproduzido quando você reconecta a uma aba.

Quando você para uma instância do Coast com `coast stop`, todos os processos PTY das shells de agente são encerrados e seus registros no banco de dados são limpos. `coast start` inicia automaticamente uma nova shell de agente se `[agent_shell]` estiver configurado.

Após uma reinicialização do daemon, shells de agente previamente em execução aparecerão como mortas. O sistema detecta isso automaticamente — se a shell ativa estiver morta, a primeira shell viva é promovida a ativa. Se nenhuma shell estiver viva, inicie uma nova com `coast agent-shell spawn --activate`.

## Para Quem Isto É

Shells de agente são projetadas para **produtos que estão construindo integrações first-party** em torno do Coasts — plataformas de orquestração, wrappers de agentes e ferramentas que querem gerenciar agentes de codificação containerizados programaticamente via as APIs `input`, `read-output` e `session-status`.

Para codificação geral com agentes paralelos, execute agentes no host. É mais simples, evita problemas de OAuth, contorna a complexidade de montagem de credenciais e aproveita ao máximo o filesystem compartilhado. Você obtém todos os benefícios do Coast (runtimes isolados, gerenciamento de portas, troca de worktree) sem nenhum dos custos de sobrecarga da containerização de agentes.

O próximo nível de complexidade além de shells de agente é montar [servidores MCP](MCP_SERVERS.md) no Coast para que o agente containerizado tenha acesso a ferramentas. Isso amplia ainda mais a superfície de integração e é coberto separadamente. A capacidade existe se você precisar, mas a maioria dos usuários não deve.
