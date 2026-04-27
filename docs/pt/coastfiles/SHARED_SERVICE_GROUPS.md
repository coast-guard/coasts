# Coastfile.shared_service_groups

`Coastfile.shared_service_groups` é um Coastfile tipado que declara os serviços que o Grupo de Serviços Compartilhados (SSG) do seu projeto executará. Ele fica ao lado de um `Coastfile` comum, e o nome do projeto vem de `[coast].name` nesse arquivo irmão -- você não o repete aqui. Cada projeto tem exatamente um desses arquivos (na sua worktree); o contêiner `<project>-ssg` executa os serviços que ele declara. Outros Coastfiles consumidores no mesmo projeto podem referenciar esses serviços com `[shared_services.<name>] from_group = true`.

Para o conceito, ciclo de vida, volumes, secrets e conexão do consumidor, veja a [documentação de Shared Service Groups](../shared_service_groups/README.md).

## Discovery

`coast ssg build` encontra o arquivo usando as mesmas regras que `coast build`:

- Padrão: procura por `Coastfile.shared_service_groups` ou `Coastfile.shared_service_groups.toml` no diretório de trabalho atual. Ambos os formatos são equivalentes; a variante `.toml` vence quando ambos existem.
- `-f <path>` / `--file <path>` aponta para um arquivo arbitrário.
- `--working-dir <dir>` desacopla a raiz do projeto da localização do Coastfile.
- `--config '<toml>'` aceita TOML inline para fluxos roteirizados.

## Accepted Sections

Apenas `[ssg]`, `[shared_services.<name>]`, `[secrets.<name>]` e `[unset]` são aceitos. Qualquer outra chave de nível superior (`[coast]`, `[ports]`, `[services]`, `[volumes]`, `[assign]`, `[omit]`, `[inject]`, ...) é rejeitada durante o parse.

`[ssg] extends = "<path>"` e `[ssg] includes = ["<path>", ...]` são suportados para composição. Veja [Inheritance](#inheritance) abaixo.

## `[ssg]`

Configuração de nível superior do SSG.

```toml
[ssg]
runtime = "dind"
```

### `runtime` (optional)

Runtime de contêiner para o DinD externo do SSG. `dind` é o único valor suportado atualmente; o campo é opcional e o padrão é `dind`.

## `[shared_services.<name>]`

Um bloco por serviço. A chave TOML (`postgres`, `redis`, ...) se torna o nome do serviço que os Coastfiles consumidores referenciam.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

### `image` (required)

A imagem Docker a ser executada dentro do daemon Docker interno do SSG. Qualquer imagem pública ou privada que o host possa baixar é aceita.

### `ports`

Portas de contêiner nas quais o serviço escuta. **Apenas inteiros simples.**

```toml
ports = [5432]
ports = [5432, 5433]
```

- Um mapeamento `"HOST:CONTAINER"` (`"5432:5432"`) é **rejeitado**. Publicações no host do SSG são sempre dinâmicas -- você nunca escolhe a porta do host.
- Um array vazio (ou o campo totalmente omitido) é permitido. Sidecars sem portas expostas são aceitáveis.

Cada porta se torna um mapeamento `PUBLISHED:CONTAINER` no DinD externo no momento de `coast ssg run`, onde `PUBLISHED` é uma porta de host alocada dinamicamente. Uma porta virtual separada por projeto é alocada para roteamento estável do consumidor -- veja [Routing](../shared_service_groups/ROUTING.md).

### `env`

Mapa plano de strings encaminhado literalmente para o ambiente do contêiner de serviço interno.

```toml
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "app" }
```

Os valores de env **não** são capturados no manifesto de build. Apenas as chaves são registradas, em conformidade com a postura de segurança de `coast build`.

Para valores que você não quer codificar no Coastfile (senhas, tokens de API), use a seção `[secrets.*]` descrita abaixo -- ela extrai do seu host no momento do build e injeta no momento da execução.

### `volumes`

Array de strings de volume no estilo Docker Compose. Cada entrada é uma destas:

```toml
volumes = [
    "/var/coast-data/postgres:/var/lib/postgresql/data",   # montagem bind do host
    "pg_wal:/var/lib/postgresql/wal",                       # volume nomeado interno
]
```

**Montagem bind do host** -- a origem começa com `/`. Os bytes vivem no filesystem real do host. Tanto o DinD externo quanto o serviço interno fazem bind do **mesmo caminho de host em formato de string**. Veja [Volumes -> Symmetric-Path Plan](../shared_service_groups/VOLUMES.md#the-symmetric-path-plan).

**Volume nomeado interno** -- a origem é um nome de volume Docker (sem `/`). O volume vive dentro do daemon Docker interno do SSG. Persiste entre reinicializações do SSG; opaco para o host.

Rejeitado durante o parse:

- Caminhos relativos (`./data:/...`).
- Componentes `..`.
- Volumes somente de contêiner (sem origem).
- Alvos duplicados dentro de um único serviço.

### `auto_create_db`

Quando `true`, o daemon cria um banco de dados `{instance}_{project}` dentro deste serviço para cada Coast consumidor que é executado. Aplica-se apenas a imagens de banco de dados reconhecidas (Postgres, MySQL). O padrão é `false`.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

Um Coastfile consumidor pode sobrescrever esse valor por projeto -- veja [Consuming -> auto_create_db](../shared_service_groups/CONSUMING.md#auto_create_db).

### `inject` (not allowed)

`inject` **não** é válido em definições de serviço do SSG. Injeção é uma preocupação do lado do consumidor (diferentes Coastfiles consumidores podem querer o mesmo Postgres do SSG exposto sob nomes diferentes de variáveis de ambiente). Veja [Coastfile: Shared Services](SHARED_SERVICES.md#inject) para a semântica de `inject` do lado do consumidor.

## `[secrets.<name>]`

O bloco `[secrets.*]` em `Coastfile.shared_service_groups` extrai credenciais do lado do host no momento de `coast ssg build` e as injeta nos serviços internos do SSG no momento de `coast ssg run`. O schema espelha o `[secrets.*]` do Coastfile comum (veja [Secrets](SECRETS.md) para a referência dos campos); o comportamento específico de SSG está documentado em [SSG Secrets](../shared_service_groups/SECRETS.md).

```toml
[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"

[secrets.tls_cert]
extractor = "file"
path = "/Users/me/certs/dev.pem"
inject = "file:/etc/ssl/certs/server.pem"
```

Os mesmos extractors estão disponíveis (`env`, `file`, `command`, `keychain`, `coast-extractor-<name>` customizado). A diretiva `inject` seleciona se o valor chega como uma variável de ambiente ou como um arquivo dentro do contêiner de serviço interno do SSG.

Por padrão, um secret nativo do SSG é injetado em **todos** os `[shared_services.*]` declarados. Para direcionar a um subconjunto, liste os nomes dos serviços explicitamente:

```toml
[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"
services = ["postgres"]      # montado apenas no serviço postgres
```

Os valores extraídos dos secrets são armazenados criptografados em `~/.coast/keystore.db` sob `coast_image = "ssg:<project>"` -- um namespace separado das entradas normais do keystore do Coast. Veja [SSG Secrets](../shared_service_groups/SECRETS.md) para o ciclo de vida completo, incluindo o verbo `coast ssg secrets clear`.

## Inheritance

Coastfiles de SSG suportam o mesmo mecanismo `extends` / `includes` / `[unset]` que Coastfiles comuns. Veja [Coastfile Inheritance](INHERITANCE.md) para o modelo mental compartilhado; esta seção documenta o formato específico de SSG.

### `[ssg] extends` -- pull in a parent Coastfile

```toml
[ssg]
extends = "Coastfile.ssg-base"

[shared_services.postgres]
image = "postgres:17-alpine"
```

O arquivo pai é resolvido em relação ao diretório pai do arquivo filho. O desempate `.toml` se aplica (o parser tenta `Coastfile.ssg-base.toml` primeiro, depois `Coastfile.ssg-base` simples). Caminhos absolutos também são aceitos.

### `[ssg] includes` -- merge fragment files

```toml
[ssg]
includes = ["dev-seed.toml", "extra-caches.toml"]

[shared_services.postgres]
image = "postgres:16-alpine"
```

Os fragmentos são mesclados em ordem antes do próprio arquivo que os inclui. Os caminhos dos fragmentos são resolvidos em relação ao diretório pai do arquivo que inclui (sem desempate `.toml` -- fragmentos normalmente são nomeados exatamente).

**Os fragmentos não podem usar `extends` ou `includes`.** Eles devem ser autocontidos.

### Merge semantics

- **Escalares de `[ssg]`** (`runtime`) -- o filho vence quando presente, caso contrário herda.
- **`[shared_services.*]`** -- substituição por nome. Se pai e filho definirem `postgres`, a entrada do filho substitui totalmente a do pai (substituição da entrada inteira, não mesclagem em nível de campo). Serviços do pai não redeclarados pelo filho são herdados.
- **`[secrets.*]`** -- substituição por nome, mesmo formato que `[shared_services.*]`. Um secret filho com o mesmo nome sobrescreve totalmente a configuração de secret do pai.
- **Ordem de carregamento** -- o pai de `extends` carrega primeiro, depois cada fragmento de `includes` em ordem, depois o próprio arquivo de nível superior. Camadas posteriores vencem em caso de colisão.

### `[unset]` -- drop inherited services or secrets

```toml
[ssg]
extends = "Coastfile.ssg-base"

[unset]
shared_services = ["mongodb"]
secrets = ["pg_password"]
```

Remove entradas nomeadas **após** a mesclagem, para que um filho possa remover seletivamente algo fornecido pelo pai. Tanto as chaves `shared_services` quanto `secrets` são suportadas.

Coastfiles SSG standalone podem tecnicamente conter `[unset]`, mas ele é silenciosamente ignorado (corresponde ao comportamento do Coastfile comum: unset só se aplica quando o arquivo participa de herança).

### Cycles

Ciclos diretos (`A` extends `B` extends `A`, ou `A` estende a si mesmo) geram erro fatal com `circular extends/includes dependency detected: '<path>'`. Herança em diamante (dois caminhos separados que ambos terminam no mesmo pai) é permitida -- o conjunto de visita é por recursão e é removido ao retornar.

### `[omit]` is not applicable

Coastfiles comuns suportam `[omit]` para remover serviços / volumes do arquivo compose. O SSG não tem um arquivo compose para remover -- ele gera compose interno diretamente a partir das entradas `[shared_services.*]`. Use `[unset]` para remover serviços herdados em vez disso.

### Inline `--config` rejects `extends` / `includes`

`coast ssg build --config '<toml>'` não pode resolver um caminho pai porque não há uma localização em disco para ancorar caminhos relativos. Passar `extends` / `includes` em TOML inline gera erro fatal com `extends and includes require file-based parsing`. Use `-f <file>` ou `--working-dir <dir>` em vez disso.

### Build artifact is the flattened form

`coast ssg build` escreve um TOML standalone em `~/.coast/ssg/<project>/builds/<id>/ssg-coastfile.toml`. O artefato contém o resultado mesclado pós-herança sem diretivas `extends`, `includes` ou `[unset]`, para que o build possa ser inspecionado ou executado novamente sem que os arquivos pai / fragmento estejam presentes. O hash `build_id` também reflete a forma achatada, então uma alteração apenas no pai invalida o cache corretamente.

## Example

Postgres + Redis com uma senha extraída do ambiente:

```toml
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast" }
auto_create_db = true

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["/var/coast-data/redis:/data"]

[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"
services = ["postgres"]
```

## See Also

- [Shared Service Groups](../shared_service_groups/README.md) -- visão geral do conceito
- [SSG Building](../shared_service_groups/BUILDING.md) -- o que `coast ssg build` faz com este arquivo
- [SSG Volumes](../shared_service_groups/VOLUMES.md) -- formatos de declaração de volume, permissões e a receita de migração de volume de host
- [SSG Secrets](../shared_service_groups/SECRETS.md) -- o pipeline de extração em build / injeção em execução para `[secrets.*]`
- [SSG Routing](../shared_service_groups/ROUTING.md) -- portas canônicas / dinâmicas / virtuais
- [Coastfile: Shared Services](SHARED_SERVICES.md) -- sintaxe do lado do consumidor para `from_group = true`
- [Coastfile: Secrets and Injection](SECRETS.md) -- a referência normal de `[secrets.*]` do Coastfile
- [Coastfile Inheritance](INHERITANCE.md) -- o modelo mental compartilhado de `extends` / `includes` / `[unset]`
