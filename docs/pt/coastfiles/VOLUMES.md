# Volumes

As seções `[volumes.*]` controlam como volumes Docker nomeados são tratados entre instâncias do Coast. Cada volume é configurado com uma estratégia que determina se as instâncias compartilham dados ou obtêm sua própria cópia independente.

Para uma visão mais ampla do isolamento de dados no Coast — incluindo serviços compartilhados como alternativa — veja [Volumes](../concepts_and_terminology/VOLUMES.md).

## Definindo um volume

Cada volume é uma seção TOML nomeada sob `[volumes]`. Três campos são obrigatórios:

- **`strategy`** — `"isolated"` ou `"shared"`
- **`service`** — o nome do serviço no compose que usa este volume
- **`mount`** — o caminho de montagem do volume no container

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"
```

## Estratégias

### `isolated`

Cada instância do Coast recebe seu próprio volume independente. Os dados não são compartilhados entre instâncias. Os volumes são criados no `coast run` e excluídos no `coast rm`.

```toml
[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"
```

Esta é a escolha certa para a maioria dos volumes de banco de dados — cada instância começa do zero e pode alterar dados livremente sem afetar outras instâncias.

### `shared`

Todas as instâncias do Coast usam um único volume Docker. Quaisquer dados gravados por uma instância ficam visíveis para todas as outras.

```toml
[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

Volumes compartilhados nunca são excluídos pelo `coast rm`. Eles persistem até que você os remova manualmente.

O Coast imprime um aviso no momento do build se você usar `shared` em um volume anexado a um serviço do tipo banco de dados. Compartilhar um único volume de banco de dados entre várias instâncias concorrentes pode causar corrupção. Se você precisa de bancos de dados compartilhados, use [shared services](SHARED_SERVICES.md) em vez disso.

Bons usos para volumes compartilhados: caches de dependências (Go modules, cache do npm, cache do pip), caches de artefatos de build e outros dados em que gravações concorrentes são seguras ou improváveis.

## Semeadura por snapshot

Volumes isolados podem ser semeados a partir de um volume Docker existente no momento da criação da instância usando `snapshot_source`. Os dados do volume de origem são copiados para o novo volume isolado, que então passa a divergir de forma independente.

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "db"
mount = "/var/lib/postgresql/data"
```

`snapshot_source` só é válido com `strategy = "isolated"`. Defini-lo em um volume compartilhado é um erro.

Isso é útil quando você quer que cada instância do Coast comece com um conjunto de dados realista copiado do banco de dados de desenvolvimento no seu host, mas você quer que as instâncias tenham liberdade para alterar esses dados sem afetar a origem ou umas às outras.

## Exemplos

### Bancos de dados isolados, cache de dependências compartilhado

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"

[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

### Stack completo semeado por snapshot

Cada instância começa com uma cópia dos volumes de banco de dados existentes no seu host e então diverge de forma independente.

```toml
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

### Executor de testes com bancos de dados limpos por instância

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
service = "mongodb"
mount = "/data/db"
```
