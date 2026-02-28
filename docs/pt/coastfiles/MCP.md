# Servidores e Clientes MCP

> **Nota:** A configuração do MCP só é relevante quando você está executando um agente de codificação dentro de um contêiner Coast via [`[agent_shell]`](AGENT_SHELL.md). Se o seu agente roda no host (a configuração mais comum), ele já tem acesso aos seus próprios servidores MCP e não precisa do Coast para configurá-los.

As seções `[mcp.*]` configuram servidores MCP (Model Context Protocol) que rodam dentro ou ao lado das suas instâncias Coast. As seções `[mcp_clients.*]` conectam esses servidores a agentes de codificação como Claude Code ou Cursor para que eles possam descobri-los e usá-los automaticamente.

Para saber como os servidores MCP são instalados, proxied e gerenciados em tempo de execução, veja [MCP Servers](../concepts_and_terminology/MCP_SERVERS.md).

## Servidores MCP — `[mcp.*]`

Cada servidor MCP é uma seção TOML nomeada sob `[mcp]`. Há dois modos: **interno** (roda dentro do contêiner Coast) e **proxied pelo host** (roda no host, com proxy para dentro do Coast).

### Servidores MCP internos

Um servidor interno é instalado e roda dentro do contêiner DinD. O campo `command` é obrigatório quando não há `proxy`.

```toml
[mcp.echo]
command = "node"
args = ["server.js"]
```

Campos:

- **`command`** (obrigatório) — o executável a ser executado
- **`args`** — argumentos passados para o comando
- **`env`** — variáveis de ambiente para o processo do servidor
- **`install`** — comandos a executar antes de iniciar o servidor (aceita uma string ou array)
- **`source`** — um diretório do host a ser copiado para dentro do contêiner em `/mcp/{name}/`

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]
```

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
```

### Servidores MCP com proxy do host

Um servidor com proxy do host roda na sua máquina host e é disponibilizado dentro do Coast via `coast-mcp-proxy`. Defina `proxy = "host"` para habilitar esse modo.

```toml
[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]
env = { MCP_MODE = "host" }
```

Quando `proxy = "host"`:

- `command`, `args` e `env` são opcionais — se omitidos, o servidor é resolvido pelo nome a partir da configuração MCP existente no host.
- `install` e `source` **não são permitidos** (o servidor roda no host, não no contêiner).

Um servidor com proxy do host sem campos adicionais procura o servidor pelo nome na configuração do host:

```toml
[mcp.host-lookup]
proxy = "host"
```

O único valor válido para `proxy` é `"host"`.

### Múltiplos servidores

Você pode definir qualquer número de servidores MCP:

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]

[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]

[mcp.host-lookup]
proxy = "host"
```

## Clientes MCP — `[mcp_clients.*]`

Conectores de cliente MCP dizem ao Coast como escrever a configuração de servidores MCP nos arquivos de configuração que agentes de codificação leem. Isso conecta automaticamente seus servidores `[mcp.*]` aos agentes.

### Conectores integrados

Dois conectores são integrados: `claude-code` e `cursor`. Usá-los não requer campos adicionais.

```toml
[mcp_clients.claude-code]
```

```toml
[mcp_clients.cursor]
```

Conectores integrados automaticamente sabem:

- **`claude-code`** — escreve em `/root/.claude/mcp_servers.json`
- **`cursor`** — escreve em `/workspace/.cursor/mcp.json`

Você pode sobrescrever o caminho de configuração:

```toml
[mcp_clients.claude-code]
config_path = "/custom/path/mcp_servers.json"
```

### Conectores personalizados

Para agentes que não são integrados, use o campo `run` para especificar um comando de shell que o Coast executa para registrar servidores MCP:

```toml
[mcp_clients.my-agent]
run = "my-agent mcp register --stdin"
```

O campo `run` não pode ser combinado com `format` ou `config_path`.

### Conectores de formato personalizado

Se o seu agente usa o mesmo formato de arquivo de configuração que Claude Code ou Cursor, mas fica em um caminho diferente:

```toml
[mcp_clients.my-agent]
format = "claude-code"
config_path = "/home/agent/.config/mcp.json"
```

O `format` deve ser `"claude-code"` ou `"cursor"`. Ao usar um nome não integrado com `format`, `config_path` é obrigatório.

## Exemplos

### Servidor MCP interno conectado ao Claude Code

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp_clients.claude-code]
```

### Servidor com proxy do host com servidor interno

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]

[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]
env = { MCP_MODE = "host" }

[mcp_clients.claude-code]
```

### Múltiplos conectores de cliente

```toml
[mcp.my-tools]
command = "my-mcp-server"
args = ["--port", "3100"]

[mcp_clients.claude-code]
[mcp_clients.cursor]
```
