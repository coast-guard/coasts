# Ciclo de Vida do SSG

O SSG de cada projeto é seu próprio contêiner externo Docker-in-Docker nomeado `<project>-ssg` (por exemplo, `cg-ssg`). Os verbos de ciclo de vida têm como alvo o SSG do projeto ao qual pertence o `Coastfile` do cwd (ou o projeto nomeado via `--working-dir`). Todo comando mutável é serializado por meio de um mutex por projeto no daemon, de modo que duas invocações concorrentes de `coast ssg run` / `coast ssg stop` contra o mesmo projeto entram em fila em vez de competir entre si -- mas dois projetos diferentes podem alterar seus SSGs em paralelo.

## Máquina de Estados

```text
                     coast ssg build           coast ssg run
(no build)   -->  built     -->     created    -->     running
                                                          |
                                                   coast ssg stop
                                                          v
                                                       stopped
                                                          |
                                                  coast ssg start
                                                          v
                                                       running
                                                          |
                                                   coast ssg rm
                                                          v
                                                      (removed)
```

- `coast ssg build` não cria um contêiner. Ele produz um artefato em disco em `~/.coast/ssg/<project>/builds/<id>/` e (quando `[secrets.*]` é declarado) extrai valores de segredos para o keystore.
- `coast ssg run` cria o DinD `<project>-ssg`, aloca portas dinâmicas no host, materializa quaisquer segredos declarados em um `compose.override.yml` por execução e inicializa a stack interna do compose.
- `coast ssg stop` para o DinD externo, mas preserva o contêiner, as linhas de portas dinâmicas e as portas virtuais por projeto para que `start` seja rápido.
- `coast ssg start` recria o SSG em execução e rematerializa segredos (assim, um `coast ssg secrets clear` entre stop e start entra em vigor).
- `coast ssg rm` remove o contêiner DinD externo. Com `--with-data` ele também remove os volumes nomeados internos (o conteúdo de bind-mounts do host nunca é tocado). O keystore nunca é limpo por `rm` -- somente `coast ssg secrets clear` faz isso.
- `coast ssg restart` é um wrapper de conveniência para `stop` + `start`.

## Comandos

### `coast ssg run`

Cria o DinD `<project>-ssg` se ele não existir e inicia seus serviços internos. Aloca uma porta dinâmica no host por serviço declarado e as publica no DinD externo. Escreve os mapeamentos no banco de estado para que o alocador de portas não as reutilize.

```bash
coast ssg run
```

Transmite eventos de progresso pelo mesmo canal `BuildProgressEvent` que `coast ssg build`. O plano padrão tem 7 etapas:

1. Preparando SSG
2. Criando contêiner SSG
3. Iniciando contêiner SSG
4. Aguardando daemon interno
5. Carregando imagens em cache
6. Materializando segredos (silencioso quando não há bloco `[secrets]`; emite itens por segredo caso contrário)
7. Iniciando serviços internos

**Auto-start**. `coast run` em um Coast consumidor que referencia um serviço SSG inicia automaticamente o SSG se ele ainda não estiver em execução. Você sempre pode executar `coast ssg run` explicitamente, mas raramente precisa. Veja [Consuming -> Auto-start](CONSUMING.md#auto-start).

### `coast ssg start`

Inicia um SSG previamente parado. Requer um contêiner `<project>-ssg` existente (isto é, uma execução anterior de `coast ssg run`). Rematerializa segredos a partir do keystore para que qualquer mudança desde o stop entre em vigor, então recria os socats de checkout no lado do host para quaisquer portas canônicas que tenham sido checked out antes do stop.

```bash
coast ssg start
```

### `coast ssg stop`

Para o contêiner DinD externo. A stack interna do compose cai junto com ele. O contêiner, as alocações de portas dinâmicas e as linhas de portas virtuais por projeto são preservados para que o próximo `start` seja rápido.

```bash
coast ssg stop
coast ssg stop --force
```

Os socats de checkout no lado do host são encerrados, mas suas linhas no banco de estado sobrevivem. O próximo `coast ssg start` ou `coast ssg run` os recria. Veja [Checkout](CHECKOUT.md).

**Bloqueio de consumidor remoto.** O daemon se recusa a parar o SSG enquanto qualquer Coast shadow remoto (um criado com `coast assign --remote ...`) estiver consumindo-o no momento. Passe `--force` para desmontar os túneis reversos SSH e prosseguir mesmo assim. Veja [Consuming -> Remote Coasts](CONSUMING.md#remote-coasts).

### `coast ssg restart`

Equivalente a `stop` + `start`. Preserva o contêiner e os mapeamentos de portas dinâmicas.

```bash
coast ssg restart
```

### `coast ssg rm`

Remove o contêiner DinD externo. Por padrão, isso preserva os volumes nomeados internos (Postgres WAL etc.), para que seus dados sobrevivam entre ciclos de `rm` / `run`. O conteúdo de bind-mounts do host nunca é tocado.

```bash
coast ssg rm                    # preserva volumes nomeados; preserva keystore
coast ssg rm --with-data        # também remove volumes nomeados; ainda preserva keystore
coast ssg rm --force            # prossegue apesar de consumidores remotos
```

- `--with-data` remove todos os volumes nomeados internos antes de remover o próprio DinD. Use isso quando quiser um banco de dados limpo.
- `--force` prossegue mesmo quando Coasts shadow remotos referenciam o SSG. Mesma semântica de `stop --force`.
- `rm` limpa linhas de `ssg_port_checkouts` (destrutivo para os bindings no host de portas canônicas).

O keystore -- onde vivem os segredos nativos do SSG (`coast_image = "ssg:<project>"`) -- **não** é afetado por `rm` ou `rm --with-data`. Para apagar segredos do SSG, use `coast ssg secrets clear` (veja [Secrets](SECRETS.md)).

### `coast ssg ps`

Mostra o status dos serviços do SSG do projeto atual. Lê `manifest.json` para a configuração construída, então inspeciona o banco de estado ativo para metadados de contêineres em execução.

```bash
coast ssg ps
```

Saída após um `run` bem-sucedido:

```text
SSG build: b455787d95cfdeb_20260420061903  (project: cg, running)

  SERVICE              IMAGE                          PORT       STATUS
  postgres             postgres:16                    5432       running
  redis                redis:7-alpine                 6379       running
```

### `coast ssg ports`

Mostra o mapeamento de portas canônica / dinâmica / virtual por serviço, com uma anotação `(checked out)` quando um socat de porta canônica no lado do host está ativo para aquele serviço. A porta virtual é à qual os consumidores realmente se conectam. Veja [Routing](ROUTING.md) para detalhes.

```bash
coast ssg ports

#   SERVICE              CANONICAL       DYNAMIC         VIRTUAL    STATUS
#   postgres             5432            54201           42000      (checked out)
#   redis                6379            54202           42001
```

### `coast ssg logs`

Transmite logs do contêiner DinD externo ou de um serviço interno específico.

```bash
coast ssg logs --tail 100
coast ssg logs --service postgres --tail 50
coast ssg logs --service postgres --follow
```

- `--service <name>` tem como alvo um serviço interno pela chave do compose; sem ele você obtém o stdout do DinD externo.
- `--tail N` limita as linhas históricas (padrão 200).
- `--follow` / `-f` transmite novas linhas conforme elas chegam, até `Ctrl+C`.

### `coast ssg exec`

Executa um comando dentro do DinD externo ou de um serviço interno.

```bash
coast ssg exec -- sh
coast ssg exec --service postgres -- psql -U coast -l
```

- Sem `--service`, o comando é executado no contêiner externo `<project>-ssg`.
- Com `--service <name>`, o comando é executado dentro daquele serviço compose via `docker compose exec -T`.
- Tudo após `--` é repassado para o `docker exec` subjacente, incluindo flags.

### `coast ssg ls`

Lista todo SSG conhecido pelo daemon, em todos os projetos. Este é o único verbo que não resolve um projeto a partir do cwd; ele retorna linhas para cada entrada no estado de SSG do daemon.

```bash
coast ssg ls

#   PROJECT     STATUS     BUILD                                       SERVICES   CREATED
#   cg          running    b455787d95cfdeb_20260420061903               2          2026-04-20T06:19:03Z
#   filemap     stopped    b9b93fdb41b21337_20260418123012               3          2026-04-18T12:30:12Z
```

Útil para encontrar SSGs esquecidos de projetos antigos, ou para ver rapidamente quais projetos nesta máquina têm um SSG em qualquer estado.

## Semântica do Mutex

Todo verbo mutável de SSG (`run`/`start`/`stop`/`restart`/`rm`/`checkout`/`uncheckout`) adquire um mutex de SSG por projeto dentro do daemon antes de despachar para o handler real. Duas invocações concorrentes contra o mesmo projeto entram em fila; contra projetos diferentes elas executam em paralelo. Verbos somente leitura (`ps`/`ports`/`logs`/`exec`/`doctor`/`ls`) não adquirem o mutex.

## Integração com Coastguard

Se você estiver executando [Coastguard](../concepts_and_terminology/COASTGUARD.md), a SPA renderiza o ciclo de vida do SSG em sua própria página (`/project/<p>/ssg/local`) com abas para Exec, Ports, Services, Logs, Secrets, Stats, Images e Volumes. `CoastEvent::SsgStarting` e `CoastEvent::SsgStarted` são disparados sempre que um Coast consumidor aciona um auto-start, para que a UI possa atribuir a inicialização ao projeto que precisou dela.
