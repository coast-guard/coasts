# Serviços Compartilhados

As seções `[shared_services.*]` definem serviços de infraestrutura — bancos de dados, caches, brokers de mensagens — que rodam no daemon do Docker do host, em vez de dentro de containers individuais do Coast. Várias instâncias do Coast se conectam ao mesmo serviço compartilhado por uma rede bridge.

Para saber como os serviços compartilhados funcionam em tempo de execução, gerenciamento de ciclo de vida e solução de problemas, consulte [Serviços Compartilhados](../concepts_and_terminology/SHARED_SERVICES.md).

## Definindo um serviço compartilhado

Cada serviço compartilhado é uma seção TOML nomeada sob `[shared_services]`. O campo `image` é obrigatório; todo o resto é opcional.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image` (obrigatório)

A imagem Docker a ser executada no daemon do host.

### `ports`

Lista de portas que o serviço expõe. Usada para o roteamento na rede bridge entre o serviço compartilhado e as instâncias do Coast.

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

Os valores de porta devem ser diferentes de zero.

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

## Ciclo de vida

Os serviços compartilhados iniciam automaticamente quando a primeira instância do Coast que os referencia é executada. Eles continuam rodando através de `coast stop` e `coast rm` — remover uma instância não afeta os dados do serviço compartilhado. Somente `coast shared rm` para e remove um serviço compartilhado.

Bancos de dados por instância criados por `auto_create_db` também sobrevivem à exclusão da instância. Use `coast shared-services rm` para remover o serviço e seus dados completamente.

## Quando usar serviços compartilhados vs volumes

Use serviços compartilhados quando múltiplas instâncias do Coast precisam falar com o mesmo servidor de banco de dados (por exemplo, um Postgres compartilhado onde cada instância recebe seu próprio banco de dados). Use [estratégias de volume](VOLUMES.md) quando você quiser controlar como os dados de um serviço interno do compose são compartilhados ou isolados.

## Exemplos

### Postgres, Redis e MongoDB

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

### Postgres compartilhado mínimo

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

### Serviços compartilhados com bancos de dados criados automaticamente

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```
