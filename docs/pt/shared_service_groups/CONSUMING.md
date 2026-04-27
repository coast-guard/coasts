# Consumindo um Grupo de Serviços Compartilhados

Um Coast consumidor opta pelos serviços pertencentes ao SSG do seu projeto por serviço, usando uma flag de uma linha no `Coastfile` do consumidor. Dentro do Coast, os contêineres da aplicação ainda veem `postgres:5432`; a camada de roteamento do daemon redireciona esse tráfego para o DinD externo `<project>-ssg` do projeto por meio de uma porta virtual estável.

O SSG ao qual `from_group = true` se refere é **sempre o próprio SSG do projeto consumidor**. Não há compartilhamento entre projetos. Se o `[coast].name` do consumidor for `cg`, `from_group = true` é resolvido em relação ao `Coastfile.shared_service_groups` de `cg-ssg`.

## Sintaxe

Adicione um bloco `[shared_services.<name>]` com `from_group = true`:

```toml
# Consumer Coastfile
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true

# Optional per-project overrides:
inject = "env:DATABASE_URL"
# auto_create_db = true       # overrides the SSG service's default
```

A chave TOML (`postgres` neste exemplo) deve corresponder a um nome de serviço declarado em `Coastfile.shared_service_groups` do projeto.

## Campos Proibidos

Com `from_group = true`, os seguintes campos são rejeitados no momento do parse:

- `image`
- `ports`
- `env`
- `volumes`

Todos eles pertencem ao lado do SSG. Se algum aparecer junto com `from_group = true`, `coast build` falha com:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports, env, volumes.
```

## Overrides Permitidos

Dois campos ainda são válidos por consumidor:

- `inject` -- a variável de ambiente ou o caminho de arquivo por meio do qual a string de conexão é exposta. Projetos consumidores diferentes podem expor o mesmo formato com nomes de variáveis de ambiente diferentes.
- `auto_create_db` -- se o Coast deve criar um banco de dados por instância dentro deste serviço no momento de `coast run`. Sobrescreve o valor `auto_create_db` do próprio serviço do SSG.

## Detecção de Conflitos

Dois blocos `[shared_services.<name>]` com o mesmo nome em um único Coastfile são rejeitados no momento do parse. Essa regra permanece.

Um bloco com `from_group = true` que referencia um nome não declarado em `Coastfile.shared_service_groups` do projeto falha no momento de `coast build`:

```text
error: shared service 'postgres' has from_group = true but no service named 'postgres' is declared in Coastfile.shared_service_groups for project 'my-app'.
```

Esta é a verificação de erro de digitação. Não há uma verificação separada de "drift" em tempo de execução -- incompatibilidades de formato entre consumidor e SSG se manifestam na verificação em tempo de build, e qualquer incompatibilidade adicional em tempo de execução aparece naturalmente como um erro de conexão do ponto de vista da aplicação.

## Inicialização automática

`coast run` em um consumidor inicia automaticamente o SSG do projeto quando ele ainda não está em execução:

- O build do SSG existe, o contêiner não está em execução -> o daemon executa o equivalente a `coast ssg start` (ou `run` se o contêiner nunca foi criado), protegido pelo mutex do SSG do projeto.
- Não existe nenhum build do SSG -> erro fatal:

  ```text
  Project 'my-app' references shared service 'postgres' from the Shared Service Group, but no SSG build exists. Run `coast ssg build` in the directory containing your Coastfile.shared_service_groups.
  ```

- SSG já em execução -> no-op, `coast run` continua imediatamente.

Os eventos de progresso `SsgStarting` e `SsgStarted` são disparados no stream de execução para que o [Coastguard](../concepts_and_terminology/COASTGUARD.md) possa atribuir a inicialização ao projeto consumidor.

## Como o Roteamento Funciona

Dentro de um Coast consumidor, o contêiner da aplicação resolve `postgres:5432` para o SSG do projeto por meio de três partes:

1. **IP de alias + `extra_hosts`** adicionam `postgres -> <docker0 alias IP>` ao compose interno do consumidor, para que consultas DNS para `postgres` tenham sucesso.
2. **socat dentro do DinD** escuta em `<alias>:5432` e encaminha para `host.docker.internal:<virtual_port>`. A porta virtual é estável para `(project, service, container_port)` -- ela não muda quando o SSG é reconstruído.
3. **socat no host** em `<virtual_port>` encaminha para `127.0.0.1:<dynamic>`, onde `<dynamic>` é a porta atualmente publicada do contêiner SSG. O socat no host é atualizado quando o SSG é reconstruído; o socat dentro do DinD do consumidor nunca precisa mudar.

O código da aplicação e o DNS do compose não mudam. Migrar um projeto de Postgres inline para Postgres em SSG é uma pequena edição no Coastfile (remover `image`/`ports`/`env`, adicionar `from_group = true`) mais um rebuild.

Para o passo a passo completo salto a salto, conceitos de porta e justificativa, veja [Routing](ROUTING.md).

## `auto_create_db`

`auto_create_db = true` em um serviço Postgres ou MySQL de SSG faz com que o daemon crie um banco de dados `{instance}_{project}` dentro desse serviço para cada Coast consumidor que executa. O nome do banco de dados corresponde ao que o padrão inline `[shared_services]` produz, então as URLs de `inject` concordam com o banco de dados que `auto_create_db` cria.

A criação é idempotente. Executar `coast run` novamente em uma instância cujo banco de dados já existe é um no-op. O SQL subjacente é idêntico ao caminho inline, então a saída DDL é byte a byte a mesma independentemente de qual padrão seu projeto usa.

Um consumidor pode sobrescrever o valor `auto_create_db` do serviço do SSG:

```toml
# SSG: auto_create_db = true, but this project doesn't want per-instance DBs.
[shared_services.postgres]
from_group = true
auto_create_db = false
```

## `inject`

`inject` expõe uma string de conexão ao contêiner da aplicação. Mesmo formato de [Secrets](../coastfiles/SECRETS.md): `"env:NAME"` cria uma variável de ambiente, `"file:/path"` escreve um arquivo dentro do contêiner coast do consumidor e o monta por bind como somente leitura em cada serviço do compose interno que não foi stubado.

A string resolvida usa o nome canônico do serviço e a porta canônica, não a porta dinâmica do host. Essa invariância é o ponto principal -- os contêineres da aplicação sempre veem `postgres://coast:coast@postgres:5432/{db}` independentemente de qual porta dinâmica o SSG esteja publicando.

Tanto `env:NAME` quanto `file:/path` estão totalmente implementados.

Este `inject` é o pipeline de segredo **do lado do consumidor**: o valor é calculado a partir dos metadados canônicos do SSG no momento de `coast build` e injetado no DinD coast do consumidor. Ele é independente do pipeline `[secrets.*]` **do lado do SSG** (veja [Secrets](SECRETS.md)) que extrai valores para os *próprios* serviços do SSG consumirem.

## Coasts Remotos

Um Coast remoto (criado com `coast assign --remote ...`) alcança um SSG local por meio de um túnel SSH reverso. O daemon local gera `ssh -N -R <vport>:localhost:<vport>` da máquina remota de volta para a porta virtual local; dentro do DinD remoto, `extra_hosts: postgres: host-gateway` resolve `postgres` para o IP host-gateway do remoto, e o túnel SSH coloca o SSG local do outro lado no mesmo número de porta virtual.

Ambos os lados do túnel usam a porta **virtual**, não a porta dinâmica. Isso significa que reconstruir o SSG localmente nunca invalida o túnel remoto.

Os túneis são coalescidos por `(project, remote_host, service, container_port)` -- múltiplas instâncias consumidoras do mesmo projeto no mesmo remoto compartilham um único processo `ssh -R`. Remover um consumidor não derruba o túnel; somente a remoção do último consumidor faz isso.

Consequências práticas:

- `coast ssg stop` / `rm` recusam enquanto um shadow Coast remoto estiver consumindo o SSG no momento. O daemon lista os shadows que estão bloqueando para que você saiba o que está usando o SSG.
- `coast ssg stop --force` (ou `rm --force`) derruba primeiro o `ssh -R` compartilhado e depois prossegue. Use isso quando você aceitar que consumidores remotos perderão conectividade.

Veja [Routing](ROUTING.md) para a arquitetura completa de túnel remoto e [Remote Coasts](../remote_coasts/README.md) para a configuração mais ampla de máquina remota.

## Veja Também

- [Routing](ROUTING.md) -- conceitos de porta canônica / dinâmica / virtual e a cadeia completa de roteamento
- [Secrets](SECRETS.md) -- `[secrets.*]` nativo de SSG para credenciais do lado do serviço (ortogonal ao `inject` do lado do consumidor)
- [Coastfile: Shared Services](../coastfiles/SHARED_SERVICES.md) -- esquema completo de `[shared_services.*]` incluindo `from_group = true`
- [Lifecycle](LIFECYCLE.md) -- o que `coast run` faz nos bastidores, incluindo a inicialização automática
- [Checkout](CHECKOUT.md) -- binding no lado do host de porta canônica para ferramentas ad-hoc
- [Volumes](VOLUMES.md) -- montagens e permissões; relevante quando você reconstrói o SSG e a nova imagem do Postgres altera a propriedade do diretório de dados
