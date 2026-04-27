# Grupos de Serviços Compartilhados

Um Grupo de Serviços Compartilhados (SSG) é um contêiner Docker-in-Docker que executa os serviços de infraestrutura do seu projeto -- Postgres, Redis, MongoDB, qualquer coisa que você colocaria em `[shared_services]` -- em um só lugar, separadamente das instâncias [Coast](../concepts_and_terminology/COASTS.md) que o consomem. Cada projeto Coast recebe seu próprio SSG, chamado `<project>-ssg`, declarado por um `Coastfile.shared_service_groups` irmão do `Coastfile` do projeto.

Cada instância consumidora (`dev-1`, `dev-2`, ...) conecta-se ao SSG do seu projeto por meio de portas virtuais estáveis, para que reconstruções do SSG não causem mudanças nos consumidores. Dentro de cada Coast, o contrato permanece inalterado: `postgres:5432` resolve para o seu Postgres compartilhado, o código da aplicação não sabe que há algo especial.

## Por que um SSG

O padrão original de [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) inicia um contêiner de infraestrutura no daemon Docker do host e o compartilha entre todas as instâncias consumidoras do projeto. Isso funciona bem para um projeto. O problema começa quando você tem **dois projetos diferentes** que cada um declara um Postgres em `5432`: ambos os projetos tentam vincular a mesma porta do host e o segundo falha.

```text
Without an SSG (cross-project host-port collision):

Host Docker daemon
+-- cg-coasts-postgres            (project "cg" binds host :5432)
+-- filemap-coasts-postgres       (project "filemap" tries :5432 -- FAILS)
+-- cg-coasts-dev-1               --> cg-coasts-postgres
+-- cg-coasts-dev-2               --> cg-coasts-postgres   (siblings share fine)
```

Os SSGs resolvem isso elevando a infraestrutura de cada projeto para seu próprio DinD. O Postgres ainda escuta na canônica `:5432` -- mas dentro do SSG, não no host. O contêiner SSG é publicado em uma porta dinâmica arbitrária do host, e um socat de porta virtual gerenciado pelo daemon (na faixa `42000-43000`) faz a ponte do tráfego dos consumidores até ele. Dois projetos podem cada um ter um Postgres na canônica 5432 porque nenhum deles vincula a 5432 do host:

```text
With an SSG (per project, no cross-project collision):

Host Docker daemon
+-- cg-ssg                        (project "cg" -- DinD)
|     +-- postgres                (inner :5432, host dyn 54201, vport 42000)
|     +-- redis                   (inner :6379, host dyn 54202, vport 42001)
+-- filemap-ssg                   (project "filemap" -- DinD, no collision)
|     +-- postgres                (inner :5432, host dyn 54250, vport 42002)
|     +-- redis                   (inner :6379, host dyn 54251, vport 42003)
+-- cg-coasts-dev-1               --> hg-internal:42000 --> cg-ssg postgres
+-- cg-coasts-dev-2               --> hg-internal:42000 --> cg-ssg postgres
+-- filemap-coasts-dev-1          --> hg-internal:42002 --> filemap-ssg postgres
```

O SSG de cada projeto possui seus próprios dados, suas próprias versões de imagem e seus próprios segredos. Os dois nunca compartilham estado, nunca competem por portas e nunca veem os dados um do outro. Dentro de cada Coast consumidor, o contrato permanece inalterado: o código da aplicação se conecta a `postgres:5432` e obtém o Postgres do seu próprio projeto -- a camada de roteamento (veja [Routing](ROUTING.md)) faz o resto.

## Início Rápido

Um `Coastfile.shared_service_groups` é um irmão do `Coastfile` do projeto. O nome do projeto vem de `[coast].name` no Coastfile regular -- você não o repete.

```toml
# Coastfile.shared_service_groups
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["pg_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_DB = "app_dev" }
auto_create_db = true

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]

# Optional: extract secrets from your environment, keychain, or 1Password
# at build time and inject them into the SSG at run time. See SECRETS.md.
[secrets.pg_password]
extractor = "env"
inject = "env:POSTGRES_PASSWORD"
var = "MY_PG_PASSWORD"
```

Construa-o e execute-o:

```bash
coast ssg build       # parse, pull images, extract secrets, write artifact
coast ssg run         # start <project>-ssg, materialize secrets, compose up
coast ssg ps          # show service status
```

Aponte um Coast consumidor para ele:

```toml
# Coastfile in the same project
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true
inject = "env:DATABASE_URL"

[shared_services.redis]
from_group = true
```

Então `coast build && coast run dev-1`. O SSG é iniciado automaticamente se ainda não estiver em execução. Dentro do contêiner da aplicação de `dev-1`, `postgres:5432` resolve para o Postgres do SSG e `$DATABASE_URL` é definida como uma string de conexão canônica.

## Referência

| Page | What it covers |
|---|---|
| [Building](BUILDING.md) | `coast ssg build` ponta a ponta, o layout de artefatos por projeto, extração de segredos, as regras de descoberta de `Coastfile.shared_service_groups` e como fixar um projeto a uma compilação específica |
| [Lifecycle](LIFECYCLE.md) | `run` / `start` / `stop` / `restart` / `rm` / `ps` / `logs` / `exec`, o contêiner `<project>-ssg` por projeto, início automático em `coast run` e `coast ssg ls` para listagem entre projetos |
| [Routing](ROUTING.md) | Portas canônicas / dinâmicas / virtuais, a camada socat do host, a cadeia completa salto a salto do app até o serviço interno e túneis simétricos para consumidores remotos |
| [Volumes](VOLUMES.md) | Bind mounts do host, caminhos simétricos, volumes nomeados internos, permissões, o comando `coast ssg doctor` e migração de um volume de host existente para dentro do SSG |
| [Consuming](CONSUMING.md) | `from_group = true`, campos permitidos e proibidos, detecção de conflito, `auto_create_db`, `inject` e consumidores remotos |
| [Secrets](SECRETS.md) | `[secrets.<name>]` no Coastfile do SSG, o pipeline de extração em tempo de compilação, injeção em tempo de execução via `compose.override.yml` e o verbo `coast ssg secrets clear` |
| [Checkout](CHECKOUT.md) | `coast ssg checkout` / `uncheckout` para vincular as portas canônicas do SSG no host para que qualquer coisa no seu host (psql, redis-cli, IDE) possa alcançá-las |
| [CLI](CLI.md) | Resumo em uma linha de cada subcomando `coast ssg` |

## Veja também

- [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) -- o padrão inline-por-instância que o SSG generaliza
- [Shared Services Coastfile reference](../coastfiles/SHARED_SERVICES.md) -- sintaxe TOML do lado do consumidor, incluindo `from_group`
- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) -- esquema completo para `Coastfile.shared_service_groups`
- [Ports](../concepts_and_terminology/PORTS.md) -- portas canônicas vs dinâmicas
