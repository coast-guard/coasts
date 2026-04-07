# Remotos

Um coast remoto executa seus serviços em uma máquina remota em vez do seu laptop. A experiência de CLI e UI é idêntica à dos coasts locais -- `coast run`, `coast assign`, `coast exec`, `coast ps` e `coast checkout` funcionam todos da mesma forma. O daemon detecta que a instância é remota e roteia as operações por meio de um túnel SSH para `coast-service` no host remoto.

## Local vs Remoto

| | Coast Local | Coast Remoto |
|---|---|---|
| Contêiner DinD | Executa na sua máquina | Executa na máquina remota |
| Serviços Compose | Dentro do DinD local | Dentro do DinD remoto |
| Edição de arquivos | Montagem bind direta | Shell coast (local) + sincronização rsync/mutagen |
| Acesso a portas | Encaminhador `socat` | Túnel SSH `-L` + encaminhador `socat` |
| Serviços compartilhados | Rede bridge | Túnel reverso SSH `-R` |
| Arquitetura de build | Arquitetura da sua máquina | Arquitetura da máquina remota |

## Como Funciona

Todo coast remoto cria dois contêineres:

1. Um **shell coast** na sua máquina local. Este é um contêiner Docker leve (`sleep infinity`) com as mesmas montagens bind de um coast normal (`/host-project`, `/workspace`). Ele existe para que agentes do host possam editar arquivos que são sincronizados com o remoto.

2. Um **coast remoto** na máquina remota, gerenciado por `coast-service`. Ele executa o contêiner DinD real com seus serviços compose, usando portas dinâmicas.

O daemon os conecta com túneis SSH:

- **Túneis de encaminhamento** (`ssh -L`): mapeiam cada porta dinâmica local para a porta dinâmica remota correspondente, para que `localhost:{dynamic}` alcance o serviço remoto.
- **Túneis reversos** (`ssh -R`): expõem [serviços compartilhados](SHARED_SERVICES.md) locais (Postgres, Redis) ao contêiner DinD remoto.

## Registrando Remotos

Os remotos são registrados no daemon e armazenados em `state.db`:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/coast_key
coast remote test my-vm
coast remote ls
coast remote rm my-vm
```

Os detalhes da conexão (host, usuário, porta, chave SSH) ficam no banco de dados do daemon, não no seu Coastfile. O Coastfile declara apenas preferências de sincronização por meio da seção `[remote]`.

## Builds Remotos

Os builds acontecem na máquina remota para que as imagens usem a arquitetura nativa do remoto. Um Mac ARM pode fazer build de imagens x86_64 em um remoto x86_64 sem compilação cruzada.

Após o build, o artefato é transferido de volta para sua máquina local para reutilização. Se outro remoto tiver a mesma arquitetura, o artefato pré-construído pode ser implantado diretamente sem reconstrução. Veja [Builds](BUILDS.md) para mais informações sobre como os artefatos de build são estruturados.

## Sincronização de Arquivos

Coasts remotos usam rsync para a transferência inicial em massa e mutagen para sincronização contínua em tempo real. Ambas as ferramentas são executadas dentro de contêineres coast (o shell coast e a imagem coast-service), não na sua máquina host. Veja o guia [Remote Coasts](../remote_coasts/README.md) para detalhes sobre a configuração de sincronização.

## Gerenciamento de Disco

Máquinas remotas acumulam volumes Docker, diretórios de workspace e tarballs de imagens. Quando `coast rm` remove uma instância remota, todos os recursos associados são limpos. Para recursos órfãos de operações com falha, use `coast remote prune`.

## Configuração

Para instruções completas de configuração, incluindo requisitos do host, implantação do coast-service e configuração do Coastfile, veja o guia [Remote Coasts](../remote_coasts/README.md).
