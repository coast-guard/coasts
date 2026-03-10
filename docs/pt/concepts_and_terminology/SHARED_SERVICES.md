# Serviços Compartilhados

Serviços compartilhados são contêineres de banco de dados e infraestrutura (Postgres, Redis, MongoDB, etc.) que são executados no daemon Docker do seu host em vez de dentro de uma Coast. As instâncias Coast se conectam a eles por uma rede bridge, então toda Coast fala com o mesmo serviço no mesmo volume do host.

![Shared services in Coastguard](../../assets/coastguard-shared-services.png)
*A aba de serviços compartilhados do Coastguard mostrando Postgres, Redis e MongoDB gerenciados pelo host.*

## Como Eles Funcionam

Quando você declara um serviço compartilhado no seu Coastfile, o Coast o inicia no daemon do host e o remove da stack compose que é executada dentro de cada contêiner Coast. As Coasts então são configuradas para rotear o tráfego com nome de serviço de volta para o contêiner compartilhado, preservando a porta do lado do contêiner do serviço dentro da Coast.

```text
Host Docker daemon
  |
  +--> postgres (host volume: infra_postgres_data)
  +--> redis    (host volume: infra_redis_data)
  +--> mongodb  (host volume: infra_mongodb_data)
  |
  +--> Coast: dev-1  --bridge network--> host postgres, redis, mongodb
  +--> Coast: dev-2  --bridge network--> host postgres, redis, mongodb
```

Como os serviços compartilhados reutilizam seus volumes de host existentes, quaisquer dados que você já tenha de executar `docker-compose up` localmente ficam imediatamente disponíveis para suas Coasts.

Essa distinção é importante quando você usa portas mapeadas:

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- No host, o serviço compartilhado é publicado em `localhost:5433`.
- Dentro de cada Coast, os contêineres da aplicação ainda se conectam a `postgis:5432`.
- Um inteiro simples como `5432` é uma abreviação para o mapeamento de identidade `"5432:5432"`.

## Quando Usar Serviços Compartilhados

- Seu projeto tem integrações MCP que se conectam a um banco de dados local — serviços compartilhados permitem que elas continuem funcionando sem descoberta dinâmica de portas. Se você publicar o serviço compartilhado na mesma porta do host que suas ferramentas já usam (por exemplo `ports = [5432]`), essas ferramentas continuam funcionando sem alterações. Se você publicá-lo em uma porta diferente do host (por exemplo `"5433:5432"`), as ferramentas do lado do host devem usar essa porta do host enquanto as Coasts continuam usando a porta do contêiner.
- Você quer instâncias Coast mais leves, já que elas não precisam executar seus próprios contêineres de banco de dados.
- Você não precisa de isolamento de dados entre instâncias Coast (cada instância vê os mesmos dados).
- Você está executando agentes de programação no host (veja [Filesystem](FILESYSTEM.md)) e quer que eles acessem o estado do banco de dados sem rotear por [`coast exec`](EXEC_AND_DOCKER.md). Com serviços compartilhados, as ferramentas de banco de dados e MCPs existentes do agente funcionam sem alterações.

Veja a página [Topologia de Volumes](VOLUMES.md) para alternativas quando você precisar de isolamento.

## Aviso de Desambiguação de Volumes

Os nomes de volumes Docker nem sempre são globalmente únicos. Se você executar `docker-compose up` a partir de vários projetos diferentes, os volumes do host aos quais o Coast anexa serviços compartilhados podem não ser os que você espera.

Antes de iniciar Coasts com serviços compartilhados, certifique-se de que o último `docker-compose up` que você executou foi do projeto que pretende usar com Coasts. Isso garante que os volumes do host correspondam ao que seu Coastfile espera.

## Solução de Problemas

Se seus serviços compartilhados parecerem estar apontando para o volume de host errado:

1. Abra a interface [Coastguard](COASTGUARD.md) (`coast ui`).
2. Navegue até a aba **Shared Services**.
3. Selecione os serviços afetados e clique em **Remove**.
4. Clique em **Refresh Shared Services** para recriá-los a partir da configuração atual do seu Coastfile.

Isso desmonta e recria os contêineres de serviço compartilhado, reconectando-os aos volumes corretos do host.
