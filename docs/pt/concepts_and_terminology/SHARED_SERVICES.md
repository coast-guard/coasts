# Serviços Compartilhados

Serviços compartilhados são contêineres de banco de dados e infraestrutura (Postgres, Redis, MongoDB, etc.) que são executados no daemon Docker do seu host em vez de dentro de um Coast. As instâncias do Coast se conectam a eles por uma rede bridge, então todo Coast fala com o mesmo serviço no mesmo volume do host.

![Serviços compartilhados no Coastguard](../../assets/coastguard-shared-services.png)
*A aba de serviços compartilhados do Coastguard mostrando Postgres, Redis e MongoDB gerenciados pelo host.*

## Como Funcionam

Quando você declara um serviço compartilhado no seu Coastfile, o Coast o inicia no daemon do host e o remove da stack compose que é executada dentro de cada contêiner Coast. Os Coasts são então configurados para rotear o tráfego do nome do serviço de volta para o contêiner compartilhado, preservando a porta do lado do contêiner do serviço dentro do Coast.

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

Como os serviços compartilhados reutilizam seus volumes de host existentes, quaisquer dados que você já tenha de executar `docker-compose up` localmente ficam imediatamente disponíveis para seus Coasts.

Essa distinção importa quando você usa portas mapeadas:

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- No host, o serviço compartilhado é publicado em `localhost:5433`.
- Dentro de cada Coast, os contêineres da aplicação ainda se conectam a `postgis:5432`.
- Um inteiro simples como `5432` é uma forma abreviada para o mapeamento de identidade `"5432:5432"`.

## Quando Usar Serviços Compartilhados

- Seu projeto tem integrações MCP que se conectam a um banco de dados local — os serviços compartilhados permitem que elas continuem funcionando sem descoberta dinâmica de portas. Se você publicar o serviço compartilhado na mesma porta do host que suas ferramentas já usam (por exemplo `ports = [5432]`), essas ferramentas continuarão funcionando sem alterações. Se você publicá-lo em uma porta diferente do host (por exemplo `"5433:5432"`), as ferramentas do lado do host devem usar essa porta do host enquanto os Coasts continuam usando a porta do contêiner.
- Você quer instâncias Coast mais leves, já que elas não precisam executar seus próprios contêineres de banco de dados.
- Você não precisa de isolamento de dados entre instâncias Coast (cada instância vê os mesmos dados).
- Você está executando agentes de programação no host (veja [Filesystem](FILESYSTEM.md)) e quer que eles acessem o estado do banco de dados sem roteamento por meio de [`coast exec`](EXEC_AND_DOCKER.md). Com serviços compartilhados, as ferramentas de banco de dados e MCPs existentes do agente funcionam sem alterações.

Veja a página [Topologia de Volumes](VOLUMES.md) para alternativas quando você precisar de isolamento.

## Aviso de Desambiguação de Volumes

Os nomes dos volumes Docker nem sempre são globalmente únicos. Se você executar `docker-compose up` a partir de vários projetos diferentes, os volumes do host aos quais o Coast anexa serviços compartilhados podem não ser os que você espera.

Antes de iniciar Coasts com serviços compartilhados, certifique-se de que o último `docker-compose up` que você executou foi do projeto que pretende usar com Coasts. Isso garante que os volumes do host correspondam ao que seu Coastfile espera.

## Solução de Problemas

Se seus serviços compartilhados parecerem estar apontando para o volume de host errado:

1. Abra a interface do [Coastguard](COASTGUARD.md) (`coast ui`).
2. Navegue até a aba **Serviços Compartilhados**.
3. Selecione os serviços afetados e clique em **Remove**.
4. Clique em **Refresh Shared Services** para recriá-los a partir da configuração atual do seu Coastfile.

Isso derruba e recria os contêineres de serviço compartilhado, reconectando-os aos volumes corretos do host.

## Serviços Compartilhados e Coasts Remotos

Ao executar [coasts remotos](REMOTES.md), os serviços compartilhados ainda são executados na sua máquina local. O daemon estabelece túneis reversos SSH (`ssh -R`) para que os contêineres DinD remotos possam alcançá-los via `host.docker.internal`. Isso mantém seu banco de dados local compartilhado com instâncias remotas. O sshd do host remoto deve ter `GatewayPorts clientspecified` habilitado para que os túneis reversos façam bind corretamente.
