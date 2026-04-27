# Serviços Compartilhados

As seções `[shared_services.*]` definem serviços de infraestrutura — bancos de dados, caches, brokers de mensagens — que um projeto Coast consome. Há duas modalidades:

- **Inline** -- declare `image`, `ports`, `env`, `volumes` diretamente no Coastfile consumidor. O Coast inicia um container no host e roteia o tráfego do app do consumidor para ele. Ideal para projetos solo com uma única instância consumidora, ou para serviços muito leves.
- **De um Grupo de Serviços Compartilhados (`from_group = true`)** -- o serviço vive no [Grupo de Serviços Compartilhados](../shared_service_groups/README.md) do projeto (um container DinD separado declarado em `Coastfile.shared_service_groups`). O Coastfile consumidor apenas faz a adesão. Ideal quando você quer extração de segredos, checkout no host para portas canônicas, ou executa múltiplos projetos Coast neste host e cada um precisa da mesma porta canônica (um SSG mantém o Postgres na porta interna `:5432` sem associar a 5432 do host, então dois projetos podem coexistir).

As duas metades desta página documentam cada modalidade por vez.

Para saber como os serviços compartilhados funcionam em tempo de execução, gerenciamento de ciclo de vida e solução de problemas, consulte [Serviços Compartilhados (conceito)](../concepts_and_terminology/SHARED_SERVICES.md).

---

## Serviços compartilhados inline

Cada serviço inline é uma seção TOML nomeada sob `[shared_services]`. O campo `image` é obrigatório; todo o resto é opcional.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image` (obrigatório)

A imagem Docker a ser executada no daemon do host.

### `ports`

Lista de portas que o serviço expõe. O Coast aceita tanto portas simples do container quanto mapeamentos no estilo Docker Compose `"HOST:CONTAINER"`.

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- Um inteiro simples como `6379` é uma forma abreviada de `"6379:6379"`.
- Uma string mapeada como `"5433:5432"` publica o serviço compartilhado na porta do host
  `5433`, enquanto o mantém acessível dentro dos Coasts em `service-name:5432`.
- As portas do host e do container devem ambas ser diferentes de zero.

### `volumes`

Strings de bind de volume do Docker para persistir dados. Esses são volumes do Docker no nível do host, não volumes gerenciados pelo Coast.

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
```

### `env`

Variáveis de ambiente passadas para o container do serviço.

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_DB = "mydb" }
```

### `auto_create_db`

Quando `true`, o Coast cria automaticamente um banco de dados por instância dentro do serviço compartilhado para cada instância do Coast. O padrão é `false`.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
auto_create_db = true
```

### `inject`

Injeta as informações de conexão do serviço compartilhado nas instâncias do Coast como uma variável de ambiente ou arquivo. Usa o mesmo formato `env:NAME` ou `file:/path` que [segredos](SECRETS.md).

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
inject = "env:DATABASE_URL"
```

### Ciclo de vida

Os serviços compartilhados inline iniciam automaticamente quando a primeira instância do Coast que os referencia é executada. Eles continuam rodando através de `coast stop` e `coast rm` — remover uma instância não afeta os dados do serviço compartilhado. Somente `coast shared rm` para e remove o serviço.

Bancos de dados por instância criados por `auto_create_db` também sobrevivem à exclusão da instância. Use `coast shared-services rm` para remover o serviço e seus dados completamente.

### Exemplos inline

#### Postgres, Redis e MongoDB

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_MULTIPLE_DATABASES = "dev_db,test_db" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis_data:/data"]

[shared_services.mongodb]
image = "mongo:latest"
ports = [27017]
volumes = ["infra_mongodb_data:/data/db"]
env = { MONGO_INITDB_ROOT_USERNAME = "myapp", MONGO_INITDB_ROOT_PASSWORD = "myapp_pass" }
```

#### Postgres compartilhado mínimo

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### Postgres com mapeamento host/container

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = ["5433:5432"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### Bancos de dados criados automaticamente

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

---

## Serviços compartilhados de um Grupo de Serviços Compartilhados

Para projetos que querem uma configuração estruturada de infraestrutura compartilhada — múltiplas worktrees, checkout no host, segredos nativos de SSG, portas virtuais através de rebuilds do SSG — declare os serviços uma vez em um [`Coastfile.shared_service_groups`](SHARED_SERVICE_GROUPS.md) e referencie-os do Coastfile consumidor com `from_group = true`:

```toml
[shared_services.postgres]
from_group = true

# Optional per-consumer overrides:
inject = "env:DATABASE_URL"
# auto_create_db = false    # overrides the SSG service's default
```

A chave TOML (`postgres` neste exemplo) deve corresponder a um serviço declarado no `Coastfile.shared_service_groups` do projeto. O SSG referenciado aqui é **sempre o SSG do próprio projeto consumidor** (chamado `<project>-ssg`, onde `<project>` é o `[coast].name` do consumidor).

### Campos proibidos com `from_group = true`

Os campos a seguir são rejeitados em tempo de parse porque o SSG é a única fonte da verdade:

- `image`
- `ports`
- `env`
- `volumes`

Qualquer um deles junto com `from_group = true` produz:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports, env, volumes.
```

### Overrides permitidos por consumidor

- `inject` -- a variável de ambiente ou caminho de arquivo através do qual a string de conexão é exposta. Diferentes Coastfiles consumidores podem expor o mesmo Postgres do SSG sob nomes de variável de ambiente diferentes.
- `auto_create_db` -- se o Coast deve criar um banco de dados por instância dentro deste serviço no momento de `coast run`. Substitui o valor `auto_create_db` do próprio serviço no SSG.

### Erro de serviço ausente

Se você referenciar um nome que não esteja declarado no `Coastfile.shared_service_groups` do projeto, `coast build` falha:

```text
error: shared service 'postgres' has from_group = true but no service named 'postgres' is declared in Coastfile.shared_service_groups for project 'my-app'.
```

### Quando escolher `from_group` em vez de inline

| Need | Inline | `from_group` |
|---|---|---|
| Single Coast project on this host, no secrets | Either works; inline is simpler | OK |
| Multiple worktrees / consumer instances of the **same** project sharing one Postgres | Works (siblings share one host container) | Works |
| **Two different Coast projects** on this host that each declare the same canonical port (e.g. both want Postgres on 5432) | Collides on host port; cannot run both concurrently | Required (each project's SSG owns its own inner Postgres without binding host 5432) |
| Want host-side `psql localhost:5432` via `coast ssg checkout` | -- | Required |
| Need build-time secret extraction for the service (`POSTGRES_PASSWORD` from a keychain, etc.) | -- | Required (see [SSG Secrets](../shared_service_groups/SECRETS.md)) |
| Stable consumer routing across rebuilds (virtual ports) | -- | Required (see [SSG Routing](../shared_service_groups/ROUTING.md)) |

Para a arquitetura completa do SSG, consulte [Grupos de Serviços Compartilhados](../shared_service_groups/README.md). Para a experiência no lado do consumidor, incluindo auto-start, detecção de drift e consumidores remotos, consulte [Consumo](../shared_service_groups/CONSUMING.md).

---

## Ver também

- [Serviços Compartilhados (conceito)](../concepts_and_terminology/SHARED_SERVICES.md) -- arquitetura de runtime para ambas as modalidades
- [Grupos de Serviços Compartilhados](../shared_service_groups/README.md) -- visão geral do conceito de SSG
- [Coastfile: Shared Service Groups](SHARED_SERVICE_GROUPS.md) -- o esquema do Coastfile do lado do SSG
- [Consumindo um SSG](../shared_service_groups/CONSUMING.md) -- passo a passo detalhado da semântica de `from_group = true`
