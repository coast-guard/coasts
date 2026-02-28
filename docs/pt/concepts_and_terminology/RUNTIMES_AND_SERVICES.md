# Runtimes e Serviços

Um Coast roda dentro de um runtime de contêiner — um contêiner externo que hospeda seu próprio daemon Docker (ou Podman). Os serviços do seu projeto rodam dentro desse daemon interno, completamente isolados de outras instâncias do Coast. Atualmente, **DinD (Docker-in-Docker) é o único runtime testado em produção.** No momento, recomendamos que você fique com o DinD até que o suporte a Podman e Sysbox tenha sido testado de ponta a ponta.

## Runtimes

O campo `runtime` no seu Coastfile seleciona qual runtime de contêiner dá suporte ao Coast. O padrão é `dind` e você pode omiti-lo completamente:

```toml
[coast]
name = "my-app"
runtime = "dind"
```

Três valores são aceitos: `dind`, `sysbox` e `podman`. Na prática, apenas o DinD está conectado ao daemon e foi testado de ponta a ponta.

### DinD (Docker-in-Docker)

O padrão e o único runtime que você deve usar hoje. O Coast cria um contêiner a partir da imagem `docker:dind` com o modo `--privileged` habilitado. Dentro desse contêiner, um daemon Docker completo é iniciado e seus serviços do `docker-compose.yml` rodam como contêineres aninhados.

O DinD é totalmente integrado:

- As imagens são pré-cacheadas no host e carregadas no daemon interno no `coast run`
- Imagens por instância são construídas no host e enviadas via `docker save | docker load`
- O estado do daemon interno é persistido em um volume nomeado (`coast-dind--{project}--{instance}`) em `/var/lib/docker`, então execuções subsequentes pulam completamente o carregamento de imagens
- Portas são publicadas diretamente do contêiner DinD para o host
- Overrides do Compose, bridging de rede de serviços compartilhados, injeção de segredos e estratégias de volume funcionam

### Sysbox (futuro)

Sysbox é um runtime OCI apenas para Linux que fornece contêineres rootless sem `--privileged`. Ele usaria `--runtime=sysbox-runc` em vez do modo privilegiado, o que é uma postura de segurança melhor. A implementação do trait existe na base de código, mas não está conectada ao daemon. Não funciona no macOS.

### Podman (futuro)

O Podman substituiria o daemon Docker interno por um daemon Podman rodando dentro de `quay.io/podman/stable`, usando `podman-compose` em vez de `docker compose`. A implementação do trait existe, mas não está conectada ao daemon.

Quando o suporte a Sysbox e Podman estabilizar, esta página será atualizada. Por enquanto, deixe `runtime` como `dind` ou omita-o.

## Arquitetura Docker-in-Docker

Todo Coast é um contêiner aninhado. O daemon Docker do host gerencia o contêiner DinD externo, e o daemon Docker interno dentro dele gerencia seus serviços do compose.

```text
Host machine
│
├── Docker daemon (host)
│   │
│   ├── coast container: dev-1 (docker:dind, --privileged)
│   │   │
│   │   ├── Inner Docker daemon
│   │   │   ├── web        (your app, :3000)
│   │   │   ├── postgres   (database, :5432)
│   │   │   └── redis      (cache, :6379)
│   │   │
│   │   ├── /workspace          ← bind mount of your project root
│   │   │   ├── /image-cache        ← read-only mount of ~/.coast/image-cache/
│   │   │   ├── /coast-artifact     ← read-only mount of the build artifact
│   │   │   ├── /coast-override     ← generated compose overrides
│   │   │   └── /var/lib/docker     ← named volume (inner daemon state)
│   │
│   ├── coast container: dev-2 (docker:dind, --privileged)
│   │   └── (same structure, fully isolated)
│   │
│   └── shared postgres (host-level, bridge network)
│
└── ~/.coast/
    ├── image-cache/    ← OCI tarballs shared across all projects
    └── state.db        ← instance metadata
```

Quando `coast run` cria uma instância, ele:

1. Cria e inicia o contêiner DinD no daemon do host
2. Consulta `docker info` dentro do contêiner até que o daemon interno esteja pronto (até 120 segundos)
3. Verifica quais imagens o daemon interno já tem (a partir do volume persistente `/var/lib/docker`) e carrega quaisquer tarballs ausentes do cache
4. Envia as imagens por instância construídas no host via `docker save | docker load`
5. Faz bind de `/host-project` em `/workspace` para que os serviços do compose vejam seu código-fonte
6. Executa `docker compose up -d` dentro do contêiner e aguarda até que todos os serviços estejam em execução ou saudáveis

O volume persistente `/var/lib/docker` é a principal otimização. Em um `coast run` novo, carregar imagens no daemon interno pode levar 20+ segundos. Em execuções subsequentes (mesmo após `coast rm` e executar novamente), o daemon interno já tem as imagens em cache e a inicialização cai para menos de 10 segundos.

## Serviços

Serviços são os contêineres (ou processos, no caso de [serviços bare](BARE_SERVICES.md)) rodando dentro do seu Coast. Para um Coast baseado em compose, estes são os serviços definidos no seu `docker-compose.yml`.

![Services tab in Coastguard](../../assets/coastguard-services.png)
*A aba Services no Coastguard mostrando serviços do compose, seu status, imagens e mapeamentos de portas.*

A aba Services no Coastguard mostra todos os serviços rodando dentro de uma instância do Coast:

- **Service** — o nome do serviço do compose (por exemplo, `web`, `backend`, `redis`). Clique para ver dados detalhados de inspect, logs e stats desse contêiner.
- **Status** — se o serviço está em execução, parado ou em um estado de erro.
- **Image** — a imagem Docker a partir da qual o serviço é construído.
- **Ports** — os mapeamentos de portas brutos do compose e as [portas canônicas/dinâmicas](PORTS.md) gerenciadas pelo coast. Portas dinâmicas estão sempre acessíveis; portas canônicas só roteiam para a instância [em checkout](CHECKOUT.md).

Você pode selecionar múltiplos serviços e parar, iniciar, reiniciar ou removê-los em lote pela barra de ferramentas.

Serviços que estão configurados como [serviços compartilhados](SHARED_SERVICES.md) rodam no daemon do host em vez de dentro do Coast, então não aparecem nesta lista. Eles têm sua própria aba.

## `coast ps`

O equivalente no CLI da aba Services é `coast ps`:

```bash
coast ps dev-1
```

```text
Services in coast instance 'dev-1':
  NAME                      STATUS               PORTS
  backend                   running              0.0.0.0:8080->8080/tcp, 0.0.0.0:40000->40000/tcp
  mailhog                   running              0.0.0.0:1025->1025/tcp, 0.0.0.0:8025->8025/tcp
  reach-web                 running              0.0.0.0:4000->4000/tcp
  test-redis                running              0.0.0.0:6380->6379/tcp
  web                       running              0.0.0.0:3000->3000/tcp
```

Por baixo dos panos, o daemon executa `docker compose ps --format json` dentro do contêiner DinD e analisa a saída JSON. Os resultados passam por vários filtros antes de serem retornados:

- **Serviços compartilhados** são removidos — eles rodam no host, não dentro do Coast.
- **Jobs de execução única** (serviços sem portas) ficam ocultos quando finalizam com sucesso. Se falharem, aparecem para que você possa investigar.
- **Serviços ausentes** — se um serviço de longa duração que deveria estar presente não estiver na saída, ele é adicionado com status `down` para que você saiba que algo está errado.

Para uma inspeção mais profunda, use `coast logs` para acompanhar a saída do serviço e [`coast exec`](EXEC_AND_DOCKER.md) para obter um shell dentro do contêiner Coast. Veja [Logs](LOGS.md) para todos os detalhes sobre streaming de logs e o tradeoff do MCP.

```bash
coast logs dev-1 --service web --tail 100
coast exec dev-1
```
