# Logs

Os serviços dentro de uma instância do Coast são executados em contêineres aninhados — seus serviços do compose são gerenciados por um daemon Docker interno dentro de um contêiner DinD. Isso significa que ferramentas de logging no nível do host não conseguem vê-los. Se o seu fluxo de trabalho inclui um logging MCP que lê logs do Docker no host, ele verá apenas o contêiner DinD externo, não o servidor web, banco de dados ou worker executando dentro dele.

A solução é `coast logs`. Qualquer agente ou ferramenta que precise ler a saída de serviços de uma instância do Coast deve usar o Coast CLI em vez de acessar logs do Docker no nível do host.

## The MCP Tradeoff

Se você estiver usando um agente de IA com um logging MCP (uma ferramenta que captura logs de contêineres Docker do seu host — veja [MCP Servers](MCP_SERVERS.md)), esse MCP não funcionará para serviços executando dentro de uma instância do Coast. O daemon Docker do host vê um contêiner por instância do Coast — o contêiner DinD — e seus logs são apenas a saída de inicialização do daemon Docker interno.

Para capturar os logs reais dos serviços, instrua seu agente a usar:

```bash
coast logs <instance> --service <service> --tail <lines>
```

Por exemplo, se o seu agente precisar inspecionar por que um serviço de backend está falhando:

```bash
coast logs dev-1 --service backend --tail 100
```

Isso é o equivalente a `docker compose logs`, mas roteado pelo daemon do Coast para dentro do contêiner DinD interno. Se você tiver regras de agente ou prompts de sistema que referenciem um logging MCP, você precisará adicionar uma instrução que sobrescreva esse comportamento ao trabalhar dentro de uma instância do Coast.

## `coast logs`

O CLI oferece várias formas de ler logs de uma instância do Coast:

```bash
coast logs dev-1                           # last 200 lines, all services
coast logs dev-1 --service web             # last 200 lines, web only
coast logs dev-1 --tail 50                 # last 50 lines, then follow
coast logs dev-1 --tail                    # all lines, then follow
coast logs dev-1 --service backend -f      # follow mode (stream new entries)
coast logs dev-1 --service web --tail 100  # last 100 lines + follow
```

Sem `--tail` ou `-f`, o comando retorna as últimas 200 linhas e encerra. Com `--tail`, ele transmite a quantidade de linhas solicitada e então continua acompanhando a nova saída em tempo real. `-f` / `--follow` habilita o modo de acompanhamento por si só.

A saída usa o formato de logs do compose com um prefixo de serviço em cada linha:

```text
web       | 2026/02/28 01:49:34 Listening on :3000
backend   | 2026/02/28 01:49:34 [INFO] Server started on :8080
backend   | 2026/02/28 01:49:34 [ProcessCreditsJob] starting at 2026-02-28T01:49:34Z
redis     | 1:M 28 Feb 2026 01:49:30.123 * Ready to accept connections
```

Você também pode filtrar por serviço com a sintaxe posicional legada (`coast logs dev-1 web`), mas a flag `--service` é a preferida.

## Coastguard Logs Tab

A UI web do Coastguard oferece uma experiência mais rica de visualização de logs com streaming em tempo real via WebSocket.

![Logs tab in Coastguard](../../assets/coastguard-logs.png)
*A aba Logs do Coastguard transmitindo a saída do serviço backend com filtragem por serviço e pesquisa.*

A aba Logs oferece:

- **Streaming em tempo real** — os logs chegam por uma conexão WebSocket conforme são produzidos, com um indicador de status mostrando o estado da conexão.
- **Filtro de serviço** — um dropdown preenchido a partir dos prefixos de serviço do stream de logs. Selecione um único serviço para focar na sua saída.
- **Pesquisa** — filtre as linhas exibidas por texto ou regex (ative o botão de asterisco para o modo regex). Os termos correspondentes são destacados.
- **Contagem de linhas** — mostra linhas filtradas vs linhas totais (por exemplo, "200 / 971 lines").
- **Limpar** — trunca os arquivos de log do contêiner interno e reinicia o visualizador.
- **Tela cheia** — expande o visualizador de logs para preencher a tela.

As linhas de log são renderizadas com suporte a cores ANSI, destaque de nível de log (ERROR em vermelho, WARN em âmbar, INFO em azul, DEBUG em cinza), atenuação de timestamp e badges de serviço coloridas para distinção visual entre serviços.

Serviços compartilhados executando no daemon do host têm seu próprio visualizador de logs acessível pela aba Shared Services. Veja [Shared Services](SHARED_SERVICES.md) para detalhes.

## How It Works

Quando você executa `coast logs`, o daemon executa `docker compose logs` dentro do contêiner DinD via `docker exec` e transmite a saída de volta para o seu terminal (ou para a UI do Coastguard via WebSocket).

```text
coast logs dev-1 --service web --tail 50
  │
  ├── CLI sends LogsRequest to daemon (Unix socket)
  │
  ├── Daemon resolves instance → container ID
  │
  ├── Daemon exec's into DinD container:
  │     docker compose logs --tail 50 --follow web
  │
  └── Output streams back chunk by chunk
        └── CLI prints to stdout / Coastguard renders in UI
```

Para [bare services](BARE_SERVICES.md), o daemon faz tail dos arquivos de log em `/var/log/coast-services/` em vez de chamar `docker compose logs`. O formato de saída é o mesmo (`service  | line`), então a filtragem por serviço funciona de forma idêntica em ambos os casos.

## Related Commands

- `coast ps <instance>` — verifique quais serviços estão em execução e seu status. Veja [Runtimes and Services](RUNTIMES_AND_SERVICES.md).
- [`coast exec <instance>`](EXEC_AND_DOCKER.md) — abra um shell dentro do contêiner do Coast para depuração manual.
