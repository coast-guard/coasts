# Serviços Compartilhados

Serviços compartilhados são contêineres de banco de dados e infraestrutura (Postgres, Redis, MongoDB, etc.) que rodam no daemon do Docker do seu host, em vez de dentro de uma Coast. As instâncias de Coast conectam-se a eles por uma rede bridge, então toda Coast fala com o mesmo serviço no mesmo volume do host.

![Shared services in Coastguard](../../assets/coastguard-shared-services.png)
*A aba de serviços compartilhados do Coastguard mostrando Postgres, Redis e MongoDB gerenciados pelo host.*

## Como Funcionam

Quando você declara um serviço compartilhado no seu Coastfile, a Coast o inicia no daemon do host e o remove da stack do compose que roda dentro de cada contêiner de Coast. As Coasts então são configuradas para rotear suas conexões de volta para o host.

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

Como os serviços compartilhados reutilizam seus volumes existentes do host, quaisquer dados que você já tenha por ter executado `docker-compose up` localmente ficam imediatamente disponíveis para suas Coasts.

## Quando Usar Serviços Compartilhados

- Seu projeto tem integrações MCP que se conectam a um banco de dados local — serviços compartilhados permitem que elas continuem funcionando sem reconfiguração. Um MCP de banco de dados no seu host que se conecta a `localhost:5432` continua funcionando porque o Postgres compartilhado está no host nessa mesma porta. Sem descoberta dinâmica de portas, sem reconfiguração de MCP. Veja [Servidores MCP](MCP_SERVERS.md) para mais sobre isso.
- Você quer instâncias de Coast mais leves, já que elas não precisam executar seus próprios contêineres de banco de dados.
- Você não precisa de isolamento de dados entre instâncias de Coast (toda instância vê os mesmos dados).
- Você está executando agentes de codificação no host (veja [Sistema de Arquivos](FILESYSTEM.md)) e quer que eles acessem o estado do banco de dados sem rotear por [`coast exec`](EXEC_AND_DOCKER.md). Com serviços compartilhados, as ferramentas de banco de dados e MCPs existentes do agente funcionam sem alterações.

Veja a página [Topologia de Volumes](VOLUMES.md) para alternativas quando você precisa de isolamento.

## Aviso de Desambiguação de Volume

Os nomes de volumes do Docker nem sempre são globalmente únicos. Se você executar `docker-compose up` a partir de vários projetos diferentes, os volumes do host aos quais a Coast anexa serviços compartilhados podem não ser os que você espera.

Antes de iniciar Coasts com serviços compartilhados, certifique-se de que o último `docker-compose up` que você executou foi do projeto que você pretende usar com Coasts. Isso garante que os volumes do host correspondam ao que seu Coastfile espera.

## Solução de Problemas

Se seus serviços compartilhados parecerem estar apontando para o volume do host errado:

1. Abra a UI do [Coastguard](COASTGUARD.md) (`coast ui`).
2. Navegue até a aba **Serviços Compartilhados**.
3. Selecione os serviços afetados e clique em **Remover**.
4. Clique em **Atualizar Serviços Compartilhados** para recriá-los a partir da sua configuração atual do Coastfile.

Isso derruba e recria os contêineres de serviço compartilhado, reanexando-os aos volumes corretos do host.
