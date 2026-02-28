# Topologia de Volumes

O Coast oferece três estratégias de volumes que controlam como serviços com muitos dados (bancos de dados, caches, etc.) armazenam e compartilham seus dados entre instâncias do Coast. Escolher a estratégia certa depende de quanta isolação você precisa e de quanto overhead você consegue tolerar.

## Serviços Compartilhados

[Serviços compartilhados](SHARED_SERVICES.md) rodam no daemon Docker do seu host, fora de qualquer contêiner do Coast. Serviços como Postgres, MongoDB e Redis permanecem na máquina host e as instâncias do Coast roteiam suas chamadas de volta para o host por meio de uma rede bridge.

```text
Host machine
  |
  +--> Postgres (host daemon, existing volume)
  +--> Redis (host daemon, existing volume)
  |
  +--> Coast: dev-1  --connects to--> host Postgres, host Redis
  +--> Coast: dev-2  --connects to--> host Postgres, host Redis
```

Não há isolação de dados entre instâncias — todo Coast fala com o mesmo banco de dados. Em troca, você obtém:

- Instâncias do Coast mais leves, já que não executam seus próprios contêineres de banco de dados.
- Seus volumes existentes no host são reutilizados diretamente, então quaisquer dados que você já tenha ficam disponíveis imediatamente.
- Integrações MCP que se conectam ao seu banco de dados local continuam a funcionar prontas para uso.

Isso é configurado no seu [Coastfile](COASTFILE_TYPES.md) em `[shared_services]`.

## Volumes Compartilhados

Volumes compartilhados montam um único volume Docker que é compartilhado entre todas as instâncias do Coast. Os serviços em si (Postgres, Redis, etc.) rodam dentro de cada contêiner do Coast, mas todos leem e escrevem no mesmo volume subjacente.

```text
Coast: dev-1  --mounts--> shared volume "my-project-postgres"
Coast: dev-2  --mounts--> shared volume "my-project-postgres"
```

Isso isola seus dados do Coast do que quer que esteja na sua máquina host, mas as instâncias ainda compartilham dados entre si. Isso é útil quando você quer uma separação limpa do seu ambiente de desenvolvimento no host sem o overhead de volumes por instância.

```toml
[volumes.postgres_data]
strategy = "shared"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

## Volumes Isolados

Volumes isolados dão a cada instância do Coast seu próprio volume independente. Nenhum dado é compartilhado entre instâncias nem com o host. Cada instância começa vazia (ou a partir de um snapshot — veja abaixo) e diverge de forma independente.

```text
Coast: dev-1  --mounts--> volume "dev-1-postgres"
Coast: dev-2  --mounts--> volume "dev-2-postgres"
```

Esta é a melhor escolha para projetos que dependem muito de testes de integração e precisam de verdadeira isolação de volume entre ambientes paralelos. A desvantagem é uma inicialização mais lenta e builds do Coast maiores, já que cada instância mantém sua própria cópia dos dados.

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

## Snapshotting

Tanto as estratégias compartilhada quanto isolada começam com volumes vazios por padrão. Se você quiser que as instâncias comecem com uma cópia de um volume existente do host, defina `snapshot_source` com o nome do volume Docker do qual copiar:

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

O snapshot é tirado no [momento do build](BUILDS.md). Após a criação, o volume de cada instância diverge de forma independente — mutações não se propagam de volta para a origem nem para outras instâncias.

O Coast ainda não oferece suporte a snapshotting em tempo de execução (por exemplo, tirar um snapshot de um volume a partir de uma instância em execução). Isso está planejado para uma versão futura.
