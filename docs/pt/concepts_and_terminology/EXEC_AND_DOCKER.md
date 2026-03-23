# Exec & Docker

`coast exec` coloca você em um shell dentro do contêiner DinD do Coast. Seu diretório de trabalho é `/workspace` — a [raiz do projeto montada por bind](FILESYSTEM.md) onde seu Coastfile está. Esta é a principal forma de executar comandos, inspecionar arquivos ou depurar serviços dentro de um Coast a partir da sua máquina host.

`coast docker` é o comando complementar para falar diretamente com o daemon Docker interno.

## `coast exec`

Abra um shell dentro de uma instância Coast:

```bash
coast exec dev-1
```

Isso inicia uma sessão `sh` em `/workspace`. Os contêineres Coast são baseados em Alpine, então o shell padrão é `sh`, não `bash`.

Você também pode executar um comando específico sem entrar em um shell interativo:

```bash
coast exec dev-1 ls -la
coast exec dev-1 -- npm install
coast exec dev-1 -- go test ./...
coast exec dev-1 --service web
coast exec dev-1 --service web -- php artisan test
```

Tudo após o nome da instância é passado como o comando. Use `--` para separar flags que pertencem ao seu comando das flags que pertencem a `coast exec`.

Passe `--service <name>` para direcionar a um contêiner de serviço compose específico em vez do contêiner Coast externo. Passe `--root` quando você precisar de acesso bruto como root do contêiner em vez do mapeamento padrão UID:GID do host feito pelo Coast.

### Working Directory

O shell inicia em `/workspace`, que é a raiz do seu projeto no host montada por bind dentro do contêiner. Isso significa que seu código-fonte, Coastfile e todos os arquivos do projeto estão ali:

```text
/workspace $ ls
Coastfile       README.md       apps/           packages/
Coastfile.light go.work         infra/          scripts/
Coastfile.snap  go.work.sum     package-lock.json
```

Quaisquer alterações que você fizer em arquivos sob `/workspace` são refletidas no host imediatamente — é uma montagem bind, não uma cópia.

### Interactive vs Non-Interactive

Quando stdin é um TTY (você está digitando em um terminal), `coast exec` ignora completamente o daemon e executa `docker exec -it` diretamente para passthrough completo de TTY. Isso significa que cores, movimento do cursor, autocompletar com tab e programas interativos funcionam como esperado.

Quando stdin é canalizado ou roteirizado (CI, fluxos de trabalho de agentes, `coast exec dev-1 -- some-command | grep foo`), a solicitação passa pelo daemon e retorna stdout, stderr e um código de saída estruturados.

### File Permissions

O exec é executado como o UID:GID do usuário do seu host, então os arquivos criados dentro do Coast têm a propriedade correta no host. Sem incompatibilidades de permissão entre host e contêiner.

## `coast docker`

Enquanto `coast exec` fornece um shell no próprio contêiner DinD, `coast docker` permite que você execute comandos da CLI do Docker contra o daemon Docker **interno** — aquele que gerencia seus serviços compose.

```bash
coast docker dev-1                    # defaults to: docker ps
coast docker dev-1 ps                 # same as above
coast docker dev-1 compose ps         # docker compose ps for the active Coast-managed stack
coast docker dev-1 images             # list images in the inner daemon
coast docker dev-1 compose logs web   # docker compose logs for a service
```

Cada comando que você passa recebe automaticamente o prefixo `docker`. Portanto, `coast docker dev-1 compose ps` executa `docker compose ps` dentro do contêiner Coast, falando com o daemon interno.

### `coast exec` vs `coast docker`

A distinção é o que você está direcionando:

| Command | Runs as | Target |
|---|---|---|
| `coast exec dev-1 ls /workspace` | `sh -c "ls /workspace"` no contêiner DinD | O próprio contêiner Coast (os arquivos do seu projeto, ferramentas instaladas) |
| `coast exec dev-1 --service web` | `docker exec ... sh` no contêiner de serviço interno resolvido | Um contêiner de serviço compose específico |
| `coast docker dev-1 ps` | `docker ps` no contêiner DinD | O daemon Docker interno (os contêineres de serviço compose) |
| `coast docker dev-1 compose logs web` | `docker compose logs web` no contêiner DinD | Os logs de um serviço compose específico via o daemon interno |

Use `coast exec` para trabalho no nível do projeto — executar testes, instalar dependências, inspecionar arquivos. Use `coast docker` quando você precisar ver o que o daemon Docker interno está fazendo — status de contêineres, imagens, redes, operações do compose.

## Coastguard Exec Tab

A interface web do Coastguard fornece um terminal interativo persistente conectado via WebSocket.

![Exec tab in Coastguard](../../assets/coastguard-exec.png)
*A aba Exec do Coastguard mostrando uma sessão de shell em /workspace dentro de uma instância Coast.*

O terminal é alimentado por xterm.js e oferece:

- **Sessões persistentes** — as sessões de terminal sobrevivem à navegação entre páginas e à atualização do navegador. Reconectar reproduz o buffer de rolagem para que você retome de onde parou.
- **Múltiplas abas** — abra vários shells ao mesmo tempo. Cada aba é uma sessão independente.
- **Abas de [Agent shell](AGENT_SHELLS.md)** — inicie shells dedicados de agente para agentes de codificação com IA, com rastreamento de status ativo/inativo.
- **Modo de tela cheia** — expanda o terminal para preencher a tela (Escape para sair).

Além da aba exec no nível da instância, o Coastguard também fornece acesso a terminal em outros níveis:

- **Exec de serviço** — clique em um serviço individual na aba Services para obter um shell dentro desse contêiner interno específico (isso faz um `docker exec` duplo — primeiro no contêiner DinD, depois no contêiner do serviço).
- **Exec de [serviço compartilhado](SHARED_SERVICES.md)** — obtenha um shell dentro de um contêiner de serviço compartilhado no nível do host.
- **Terminal do host** — um shell na sua máquina host na raiz do projeto, sem entrar em um Coast.

## When to Use Which

- **`coast exec`** — execute comandos no nível do projeto dentro do contêiner DinD, ou passe `--service` para abrir um shell ou executar um comando dentro de um contêiner de serviço compose específico.
- **`coast docker`** — inspecione ou gerencie o daemon Docker interno (status de contêineres, imagens, redes, operações do compose).
- **Aba Exec do Coastguard** — depuração interativa com sessões persistentes, múltiplas abas e suporte a agent shell. Ideal quando você quer manter vários terminais abertos enquanto navega pelo restante da interface.
- **`coast logs`** — para ler a saída do serviço, use `coast logs` em vez de `coast docker compose logs`. Veja [Logs](LOGS.md).
- **`coast ps`** — para verificar o status do serviço, use `coast ps` em vez de `coast docker compose ps`. Veja [Runtimes and Services](RUNTIMES_AND_SERVICES.md).
