# Exec & Docker

`coast exec` coloca você em um shell dentro do contêiner DinD do Coast. Seu diretório de trabalho é `/workspace` — a [raiz do projeto montada via bind](FILESYSTEM.md) onde seu Coastfile fica. Esta é a principal forma de executar comandos, inspecionar arquivos ou depurar serviços dentro de um Coast a partir da sua máquina host.

`coast docker` é o comando complementar para falar diretamente com o daemon Docker interno.

## `coast exec`

Abra um shell dentro de uma instância do Coast:

```bash
coast exec dev-1
```

Isso inicia uma sessão `sh` em `/workspace`. Os contêineres do Coast são baseados em Alpine, então o shell padrão é `sh`, não `bash`.

Você também pode executar um comando específico sem entrar em um shell interativo:

```bash
coast exec dev-1 ls -la
coast exec dev-1 -- npm install
coast exec dev-1 -- go test ./...
```

Tudo após o nome da instância é passado como o comando. Use `--` para separar flags que pertencem ao seu comando das flags que pertencem ao `coast exec`.

### Working Directory

O shell começa em `/workspace`, que é a raiz do seu projeto no host montada via bind dentro do contêiner. Isso significa que seu código-fonte, Coastfile e todos os arquivos do projeto estão ali:

```text
/workspace $ ls
Coastfile       README.md       apps/           packages/
Coastfile.light go.work         infra/          scripts/
Coastfile.snap  go.work.sum     package-lock.json
```

Qualquer alteração que você fizer em arquivos sob `/workspace` é refletida no host imediatamente — é uma montagem via bind, não uma cópia.

### Interactive vs Non-Interactive

Quando o stdin é um TTY (você está digitando em um terminal), `coast exec` ignora o daemon completamente e executa `docker exec -it` diretamente para passthrough total de TTY. Isso significa que cores, movimento do cursor, autocompletar com tab e programas interativos funcionam como esperado.

Quando o stdin é redirecionado (piped) ou executado via script (CI, fluxos de trabalho de agentes, `coast exec dev-1 -- some-command | grep foo`), a requisição passa pelo daemon e retorna stdout, stderr e um código de saída estruturados.

### File Permissions

O exec roda com o UID:GID do seu usuário do host, então os arquivos criados dentro do Coast têm a propriedade correta no host. Sem incompatibilidades de permissão entre host e contêiner.

## `coast docker`

Enquanto `coast exec` oferece um shell no próprio contêiner DinD, `coast docker` permite executar comandos do Docker CLI contra o daemon Docker **interno** — aquele que gerencia seus serviços do compose.

```bash
coast docker dev-1                    # defaults to: docker ps
coast docker dev-1 ps                 # same as above
coast docker dev-1 compose ps         # docker compose ps (inner services)
coast docker dev-1 images             # list images in the inner daemon
coast docker dev-1 compose logs web   # docker compose logs for a service
```

Todo comando que você passar é automaticamente prefixado com `docker`. Então `coast docker dev-1 compose ps` executa `docker compose ps` dentro do contêiner Coast, falando com o daemon interno.

### `coast exec` vs `coast docker`

A distinção é o que você está direcionando:

| Command | Runs as | Target |
|---|---|---|
| `coast exec dev-1 ls /workspace` | `sh -c "ls /workspace"` in DinD container | O próprio contêiner Coast (seus arquivos do projeto, ferramentas instaladas) |
| `coast docker dev-1 ps` | `docker ps` in DinD container | O daemon Docker interno (seus contêineres de serviço do compose) |
| `coast docker dev-1 compose logs web` | `docker compose logs web` in DinD container | Logs de um serviço específico do compose via o daemon interno |

Use `coast exec` para trabalho no nível do projeto — executar testes, instalar dependências, inspecionar arquivos. Use `coast docker` quando você precisar ver o que o daemon Docker interno está fazendo — status de contêiner, imagens, redes, operações do compose.

## Coastguard Exec Tab

A UI web do Coastguard fornece um terminal interativo persistente conectado via WebSocket.

![Exec tab in Coastguard](../../assets/coastguard-exec.png)
*A aba Exec do Coastguard mostrando uma sessão de shell em /workspace dentro de uma instância do Coast.*

O terminal é alimentado pelo xterm.js e oferece:

- **Sessões persistentes** — sessões de terminal sobrevivem à navegação de páginas e ao refresh do navegador. Ao reconectar, o buffer de scrollback é reproduzido para que você retome de onde parou.
- **Múltiplas abas** — abra vários shells ao mesmo tempo. Cada aba é uma sessão independente.
- Abas de **[Agent shell](AGENT_SHELLS.md)** — crie shells dedicados para agentes de codificação com IA, com rastreamento de status ativo/inativo.
- **Modo tela cheia** — expanda o terminal para preencher a tela (Escape para sair).

Além da aba de exec no nível da instância, o Coastguard também fornece acesso ao terminal em outros níveis:

- **Exec de serviço** — clique em um serviço individual na aba Services para obter um shell dentro daquele contêiner interno específico (isso faz um `docker exec` duplo — primeiro no contêiner DinD, depois no contêiner do serviço).
- Exec de **[Shared service](SHARED_SERVICES.md)** — obtenha um shell dentro de um contêiner de serviço compartilhado no nível do host.
- **Terminal do host** — um shell na sua máquina host na raiz do projeto, sem entrar em um Coast.

## When to Use Which

- **`coast exec`** — execute comandos no nível do projeto (npm install, go test, inspeção de arquivos, depuração) dentro do contêiner DinD.
- **`coast docker`** — inspecione ou gerencie o daemon Docker interno (status de contêiner, imagens, redes, operações do compose).
- **Aba Exec do Coastguard** — depuração interativa com sessões persistentes, múltiplas abas e suporte a agent shell. Melhor quando você quer manter vários terminais abertos enquanto navega pelo restante da UI.
- **`coast logs`** — para ler a saída do serviço, use `coast logs` em vez de `coast docker compose logs`. Veja [Logs](LOGS.md).
- **`coast ps`** — para verificar o status do serviço, use `coast ps` em vez de `coast docker compose ps`. Veja [Runtimes and Services](RUNTIMES_AND_SERVICES.md).
