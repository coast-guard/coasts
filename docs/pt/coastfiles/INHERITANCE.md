# Herança, Tipos e Composição

Os Coastfiles oferecem suporte a herança (`extends`), composição de fragmentos (`includes`), remoção de itens (`[unset]`) e exclusão em nível de compose (`[omit]`). Juntos, esses recursos permitem definir uma configuração base uma vez e criar variantes enxutas para diferentes fluxos de trabalho — executores de teste, frontends leves, stacks inicializadas por snapshot — sem duplicar configuração.

Para uma visão geral mais abrangente de como Coastfiles tipados se encaixam no sistema de build, consulte [Coastfile Types](../concepts_and_terminology/COASTFILE_TYPES.md) e [Builds](../concepts_and_terminology/BUILDS.md).

## Tipos de Coastfile

O Coastfile base é sempre chamado `Coastfile`. Variantes tipadas usam o padrão de nomenclatura `Coastfile.{type}`:

- `Coastfile` — o tipo padrão
- `Coastfile.light` — tipo `light`
- `Coastfile.snap` — tipo `snap`
- `Coastfile.ci.minimal` — tipo `ci.minimal`

Qualquer Coastfile pode ter uma extensão opcional `.toml` para realce de sintaxe no editor. O sufixo `.toml` é removido antes de extrair o tipo:

- `Coastfile.toml` = `Coastfile` (tipo padrão)
- `Coastfile.light.toml` = `Coastfile.light` (tipo `light`)
- `Coastfile.ci.minimal.toml` = `Coastfile.ci.minimal` (tipo `ci.minimal`)

Se existirem tanto a forma simples quanto a forma `.toml` (por exemplo, `Coastfile` e `Coastfile.toml`), a variante `.toml` tem precedência.

Os nomes `Coastfile.default` e `"toml"` (como um tipo) são reservados e não são permitidos. Um ponto final (`Coastfile.`) também é inválido.

Faça build e execute variantes tipadas com `--type`:

```
coast build --type light
coast run test-1 --type light
```

Cada tipo tem seu próprio pool de build independente. Um build com `--type light` não interfere nos builds padrão.

## `extends`

Um Coastfile tipado pode herdar de um pai usando `extends` na seção `[coast]`. O pai é totalmente analisado primeiro, e depois os valores do filho são aplicados por cima.

```toml
[coast]
extends = "Coastfile"
```

O valor é um caminho relativo para o Coastfile pai, resolvido em relação ao diretório do filho. Se o caminho exato não existir, o Coast também tentará adicionar `.toml` — portanto, `extends = "Coastfile"` encontrará `Coastfile.toml` se apenas a variante `.toml` existir no disco. Cadeias são suportadas — um filho pode estender um pai que por sua vez estende um avô:

```
Coastfile                    (base)
  └─ Coastfile.light         (extends Coastfile)
       └─ Coastfile.chain    (extends Coastfile.light)
```

Cadeias circulares (A estende B estende A, ou A estende A) são detectadas e rejeitadas.

### Semântica de merge

Quando um filho estende um pai:

- **Campos escalares** (`name`, `runtime`, `compose`, `root`, `worktree_dir`, `autostart`, `primary_port`) — o valor do filho vence se presente; caso contrário, é herdado do pai.
- **Mapas** (`[ports]`, `[egress]`) — mesclados por chave. Chaves do filho substituem chaves do pai com o mesmo nome; chaves exclusivas do pai são preservadas.
- **Seções nomeadas** (`[secrets.*]`, `[volumes.*]`, `[shared_services.*]`, `[mcp.*]`, `[mcp_clients.*]`, `[services.*]`) — mescladas por nome. Uma entrada do filho com o mesmo nome substitui completamente a entrada do pai; novos nomes são adicionados.
- **`[coast.setup]`**:
  - `packages` — união com deduplicação (o filho adiciona novos pacotes, os pacotes do pai são mantidos)
  - `run` — os comandos do filho são anexados após os comandos do pai
  - `files` — mesclados por `path` (mesmo caminho = a entrada do filho substitui a do pai)
- **`[inject]`** — as listas `env` e `files` são concatenadas.
- **`[omit]`** — as listas `services` e `volumes` são concatenadas.
- **`[assign]`** — substituído inteiramente se presente no filho (não é mesclado campo por campo).
- **`[agent_shell]`** — substituído inteiramente se presente no filho.

### Herdando o nome do projeto

Se o filho não definir `name`, ele herda o nome do pai. Isso é normal para variantes tipadas — elas são variantes do mesmo projeto:

```toml
# Coastfile
[coast]
name = "my-app"
```

```toml
# Coastfile.light — herda o nome "my-app"
[coast]
extends = "Coastfile"
autostart = false
```

Você pode substituir `name` no filho se quiser que a variante apareça como um projeto separado:

```toml
[coast]
extends = "Coastfile"
name = "my-app-light"
```

## `includes`

O campo `includes` mescla um ou mais arquivos de fragmento TOML no Coastfile antes que os próprios valores do arquivo sejam aplicados. Isso é útil para extrair configuração compartilhada (como um conjunto de secrets ou servidores MCP) em fragmentos reutilizáveis.

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]
```

Um fragmento incluído é um arquivo TOML com a mesma estrutura de seções de um Coastfile. Ele deve conter uma seção `[coast]` (que pode estar vazia), mas não pode usar `extends` nem `includes`.

```toml
# extra-secrets.toml
[coast]

[secrets.mongo_uri]
extractor = "env"
var = "MONGO_URI"
inject = "env:MONGO_URI"
```

Ordem de merge quando `extends` e `includes` estão ambos presentes:

1. Analisar o pai (via `extends`), recursivamente
2. Mesclar cada fragmento incluído em ordem
3. Aplicar os próprios valores do arquivo (que vencem sobre tudo)

## `[unset]`

Remove itens nomeados da configuração resolvida depois que todo o merge é concluído. É assim que um filho remove algo que herdou de seu pai sem precisar redefinir a seção inteira.

```toml
[unset]
secrets = ["db_password"]
shared_services = ["postgres", "redis"]
ports = ["postgres", "redis"]
```

Campos suportados:

- `secrets` — lista de nomes de secret a remover
- `ports` — lista de nomes de porta a remover
- `shared_services` — lista de nomes de serviço compartilhado a remover
- `volumes` — lista de nomes de volume a remover
- `mcp` — lista de nomes de servidor MCP a remover
- `mcp_clients` — lista de nomes de cliente MCP a remover
- `egress` — lista de nomes de egress a remover
- `services` — lista de nomes de serviço simples a remover

`[unset]` é aplicado após a resolução de toda a cadeia de merge de extends + includes. Ele remove itens por nome do resultado final mesclado.

## `[omit]`

Remove serviços e volumes do compose da stack Docker Compose que é executada dentro do Coast. Diferentemente de `[unset]` (que remove configuração em nível de Coastfile), `[omit]` instrui o Coast a excluir serviços ou volumes específicos ao executar `docker compose up` dentro do container DinD.

```toml
[omit]
services = ["monitoring", "debug-tools", "nginx-proxy"]
volumes = ["keycloak-db-data"]
```

- **`services`** — nomes de serviços do compose a excluir de `docker compose up`
- **`volumes`** — nomes de volumes do compose a excluir

Isso é útil quando seu `docker-compose.yml` define serviços de que você não precisa em todas as variantes do Coast — stacks de monitoramento, proxies reversos, ferramentas administrativas. Em vez de manter vários arquivos compose, você usa um único arquivo compose e remove o que não precisa por variante.

Quando um filho estende um pai, as listas de `[omit]` são concatenadas — o filho adiciona à lista de omissão do pai.

## Exemplos

### Variante leve para testes

Estende o Coastfile base, desativa o autostart, remove serviços compartilhados e executa bancos de dados isolados por instância:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "backend", "postgres", "redis"]
shared_services = ["postgres", "redis", "mongodb"]

[omit]
services = ["redis", "backend", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[assign]
default = "none"
[assign.services]
backend-test = "rebuild"
migrations = "rebuild"
```

### Variante inicializada por snapshot

Remove serviços compartilhados da base e os substitui por volumes isolados inicializados por snapshot:

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis", "mongodb"]

[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "infra_redis_data"
service = "redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
snapshot_source = "infra_mongodb_data"
service = "mongodb"
mount = "/data/db"
```

### Variante tipada com serviços compartilhados extras e includes

Estende a base, adiciona MongoDB e puxa secrets extras de um fragmento:

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]

[ports]
mongodb = 37017

[shared_services.mongodb]
image = "mongo:7"
ports = [27017]
env = { MONGO_INITDB_ROOT_USERNAME = "dev", MONGO_INITDB_ROOT_PASSWORD = "dev" }

[omit]
services = ["debug-tools"]
```

### Cadeia de herança em múltiplos níveis

Três níveis de profundidade: base -> light -> chain.

```toml
# Coastfile.chain
[coast]
extends = "Coastfile.light"

[coast.setup]
run = ["echo 'chain setup appended'"]

[ports]
debug = 39999
```

A configuração resolvida começa com o `Coastfile` base, mescla `Coastfile.light` por cima e depois mescla `Coastfile.chain` por cima disso. Os comandos `run` de setup de todos os três níveis são concatenados em ordem. Os `packages` de setup são deduplicados em todos os níveis.

### Omitindo serviços de uma grande stack compose

Remove serviços de `docker-compose.yml` que não são necessários para desenvolvimento:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[omit]
services = ["backend-debug", "backend-debug-test", "asynqmon", "postgres-keycloak", "keycloak", "redash-db-init", "redash-init", "redash", "redash-scheduler", "redash-worker", "langfuse-db-init", "langfuse", "nginx-proxy"]
volumes = ["keycloak-db-data"]

[ports]
web = 3000
backend = 8080
```
