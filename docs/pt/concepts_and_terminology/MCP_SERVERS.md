# Servidores MCP

Servidores MCP (Model Context Protocol) dão a agentes de IA acesso a ferramentas — busca de arquivos, consultas a banco de dados, consulta de documentação, automação de navegador e muito mais. O Coast pode instalar e configurar servidores MCP dentro de um contêiner do Coast para que um agente containerizado tenha acesso às ferramentas de que precisa.

**Isto só é relevante se você estiver executando seu agente dentro do contêiner do Coast.** Se você executar agentes no host (a abordagem recomendada), seus servidores MCP também rodam no host e nenhuma dessa configuração é necessária. Esta página se baseia em [Agent Shells](AGENT_SHELLS.md) e adiciona outra camada de complexidade por cima. Leia os avisos lá antes de prosseguir.

## Servidores Internos vs Proxy para Host

O Coast oferece suporte a dois modos para servidores MCP, controlados pelo campo `proxy` na seção `[mcp]` do seu Coastfile.

### Servidores Internos

Servidores internos são instalados e executados dentro do contêiner DinD em `/mcp/<name>/`. Eles têm acesso direto ao sistema de arquivos containerizado e aos serviços em execução.

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
```

Você também pode copiar arquivos-fonte do seu projeto para o diretório do MCP:

```toml
[mcp.my-custom-tool]
source = "tools/my-mcp-server"
install = ["npm install", "npm run build"]
command = "node"
args = ["dist/index.js"]
```

O campo `source` copia arquivos de `/workspace/<path>/` para `/mcp/<name>/` durante a configuração. Os comandos `install` rodam dentro desse diretório. Isso é útil para servidores MCP que vivem no seu repositório.

### Servidores com Proxy para Host

Servidores com proxy para host rodam na sua máquina host, não dentro do contêiner. O Coast gera uma configuração de cliente que usa `coast-mcp-proxy` para encaminhar requisições MCP do contêiner para o host pela rede.

```toml
[mcp.browser]
proxy = "host"
command = "npx"
args = ["@anthropic-ai/browser-mcp"]
```

Servidores com proxy para host não podem ter campos `install` ou `source` — espera-se que eles já estejam disponíveis no host. Use este modo para servidores MCP que precisam de acesso em nível de host, como automação de navegador ou ferramentas do sistema de arquivos do host.

### Quando Usar Cada Um

| Modo | Roda em | Bom para | Limitações |
|---|---|---|---|
| Interno | Contêiner DinD | Ferramentas que precisam de acesso ao sistema de arquivos do contêiner, ferramentas específicas do projeto | Deve ser instalável no Alpine Linux, adiciona tempo ao `coast run` |
| Proxy para host | Máquina host | Automação de navegador, ferramentas em nível de host, servidores grandes pré-instalados | Não pode acessar diretamente o sistema de arquivos do contêiner |

## Conectores de Cliente

A seção `[mcp_clients]` informa ao Coast onde gravar a configuração gerada do servidor MCP para que o agente dentro do contêiner consiga descobrir os servidores.

### Formatos Integrados

Para Claude Code e Cursor, uma seção vazia com o nome correto é suficiente — o Coast detecta automaticamente o formato e o caminho padrão de configuração:

```toml
[mcp_clients.claude-code]
# Writes to /root/.claude/mcp_servers.json (auto-detected)

[mcp_clients.cursor]
# Writes to /workspace/.cursor/mcp.json (auto-detected)
```

### Caminho de Configuração Personalizado

Para outras ferramentas de IA, especifique o formato e o caminho explicitamente:

```toml
[mcp_clients.my-tool]
format = "claude-code"
config_path = "/home/coast/.config/my-tool/mcp.json"
```

### Conectores Baseados em Comando

Em vez de gravar um arquivo, você pode canalizar o JSON de configuração gerado para um comando:

```toml
[mcp_clients.custom-setup]
run = "my-config-tool import-mcp --stdin"
```

O campo `run` é mutuamente exclusivo com `format` e `config_path`.

## Aba MCP no Coastguard

A interface web do [Coastguard](COASTGUARD.md) fornece visibilidade sobre sua configuração de MCP pela aba MCP.

![MCP tab in Coastguard](../../assets/coastguard-mcp.png)
*A aba MCP do Coastguard mostrando servidores configurados, suas ferramentas e locais de configuração do cliente.*

A aba tem três seções:

- **MCP Servers** — lista cada servidor declarado com seu nome, tipo (Interno ou Host), comando e status (Instalado, Com Proxy ou Não Instalado).
- **Tools** — selecione um servidor para inspecionar as ferramentas que ele expõe via protocolo MCP. Cada ferramenta mostra seu nome e descrição; clique para ver o esquema completo de entrada.
- **Client Locations** — mostra onde os arquivos de configuração gerados foram gravados (por exemplo, formato `claude-code` em `/root/.claude/mcp_servers.json`).

## Comandos de CLI

```bash
coast mcp dev-1 ls                          # list servers with type and status
coast mcp dev-1 tools context7              # list tools exposed by a server
coast mcp dev-1 tools context7 info resolve # show input schema for a specific tool
coast mcp dev-1 locations                   # show where client configs were written
```

O comando `tools` funciona enviando requisições JSON-RPC `initialize` e `tools/list` para o processo do servidor MCP dentro do contêiner. Ele só funciona para servidores internos — servidores com proxy para host devem ser inspecionados a partir do host.

## Como a Instalação Funciona

Durante `coast run`, depois que o daemon Docker interno está pronto e os serviços estão iniciando, o Coast configura o MCP:

1. Para cada servidor MCP **interno**:
   - Cria `/mcp/<name>/` dentro do contêiner DinD
   - Se `source` estiver definido, copia arquivos de `/workspace/<source>/` para `/mcp/<name>/`
   - Executa cada comando `install` dentro de `/mcp/<name>/` (por exemplo, `npm install -g @upstash/context7-mcp`)

2. Para cada **conector de cliente**:
   - Gera a configuração JSON no formato apropriado (Claude Code ou Cursor)
   - Servidores internos recebem seu `command` e `args` reais com `cwd` definido como `/mcp/<name>/`
   - Servidores com proxy para host recebem `coast-mcp-proxy` como comando com o nome do servidor como argumento
   - Grava a configuração no caminho de destino (ou a canaliza para o comando `run`)

Servidores com proxy para host dependem de `coast-mcp-proxy` dentro do contêiner para encaminhar requisições do protocolo MCP de volta para a máquina host, onde o processo real do servidor MCP é executado.

## Exemplo Completo

Um Coastfile que configura uma ferramenta interna de documentação e uma ferramenta de navegador com proxy para host, conectadas ao Claude Code:

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp.browser]
proxy = "host"
command = "npx"
args = ["@anthropic-ai/browser-mcp"]

[mcp_clients.claude-code]
```

Após `coast run`, o Claude Code dentro do contêiner vê ambos os servidores na sua configuração MCP — `context7` rodando localmente em `/mcp/context7/` e `browser` com proxy para o host.

## Agentes Executando no Host

Se o seu agente de codificação roda na máquina host (a abordagem recomendada), seus servidores MCP também rodam no host e a configuração `[mcp]` do Coast não entra em cena. No entanto, há uma coisa a considerar: **servidores MCP que se conectam a bancos de dados ou serviços dentro de um Coast precisam saber a porta correta.**

Quando serviços rodam dentro de um Coast, eles ficam acessíveis em portas dinâmicas que mudam a cada vez que você executa uma nova instância. Um MCP de banco de dados no host que se conecta a `localhost:5432` só alcançará o banco de dados do Coast [checked-out](CHECKOUT.md) — ou nada, se nenhum Coast estiver checked-out. Para instâncias não checked-out, você precisaria reconfigurar o MCP para usar a [porta dinâmica](PORTS.md) (por exemplo, `localhost:55681`).

Há duas formas de contornar isso:

**Use serviços compartilhados.** Se seu banco de dados roda como um [serviço compartilhado](SHARED_SERVICES.md), ele fica no daemon Docker do host na sua porta canônica (`localhost:5432`). Cada instância de Coast se conecta a ele por uma rede bridge, e seu MCP no host se conecta ao mesmo banco no mesmo porto de sempre. Sem necessidade de reconfiguração, sem descoberta de porta dinâmica. Esta é a abordagem mais simples.

**Use `coast exec` ou `coast docker`.** Se seu banco de dados roda dentro do Coast (volumes isolados), seu agente no host ainda pode consultá-lo executando comandos através do Coast (veja [Exec & Docker](EXEC_AND_DOCKER.md)):

```bash
coast exec dev-1 -- psql -h localhost -U myuser -d mydb -c "SELECT count(*) FROM users"
coast docker dev-1 exec -i my-postgres psql -U myuser -d mydb -c "\\dt"
```

Isso evita totalmente a necessidade de saber a porta dinâmica — o comando roda dentro do Coast, onde o banco de dados está na sua porta canônica.

Para a maioria dos fluxos de trabalho, serviços compartilhados são o caminho de menor resistência. A configuração do seu MCP no host permanece exatamente a mesma de antes de você começar a usar Coasts.
