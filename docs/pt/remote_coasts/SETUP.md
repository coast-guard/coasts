# Configuração

Esta página cobre tudo o que é necessário para colocar um coast remoto em funcionamento: preparar o host remoto, implantar o coast-service, registrar o remoto e executar seu primeiro coast remoto.

## Requisitos do Host

| Requisito | Motivo |
|---|---|
| Docker | Executa os contêineres coast-service e DinD |
| `GatewayPorts clientspecified` em sshd_config | Permite que túneis reversos SSH para [serviços compartilhados](../concepts_and_terminology/SHARED_SERVICES.md) façam bind em todas as interfaces, não apenas em localhost |
| sudo sem senha para o usuário SSH | O daemon usa `sudo rsync` e `sudo chown` para o gerenciamento de arquivos do workspace (os diretórios de workspace remotos podem pertencer ao root por operações do coast-service) |
| Bind mount para `/data` (não um volume Docker) | O daemon faz rsync de arquivos para o sistema de arquivos do host via SSH. Volumes Docker nomeados são isolados do sistema de arquivos do host e invisíveis ao rsync |
| 50 GB+ de disco | Imagens Docker existem no Docker do host, em tarballs e são carregadas em contêineres DinD. Veja [gerenciamento de disco](CLI.md#disk-management) para detalhes |
| Acesso SSH | O daemon conecta-se ao remoto via SSH para túneis, rsync e acesso à API do coast-service |

## Preparar o Host Remoto

Em uma máquina Linux nova (EC2, GCP, bare metal):

```bash
# Install Docker and git
sudo yum install -y docker git          # Amazon Linux
# sudo apt-get install -y docker.io git # Ubuntu/Debian

# Enable Docker and add your user to the docker group
sudo systemctl enable docker
sudo systemctl start docker
sudo usermod -aG docker $(whoami)

# Enable GatewayPorts for shared service tunnels
sudo sh -c 'echo "GatewayPorts clientspecified" >> /etc/ssh/sshd_config'
sudo systemctl restart sshd

# Create the data directory with correct ownership
sudo mkdir -p /data && sudo chown $(whoami):$(whoami) /data
```

Faça logout e login novamente para que a alteração no grupo docker tenha efeito.

## Implantar o coast-service

Clone o repositório e construa a imagem de produção:

```bash
git clone https://github.com/coast-guard/coasts.git
cd coasts && git checkout <branch>
docker build -t coast-service -f Dockerfile.coast-service .
```

Execute-o com um bind mount (não um volume Docker):

```bash
docker run -d \
  --name coast-service \
  --privileged \
  -p 31420:31420 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v /data:/data \
  coast-service
```

Verifique se está em execução:

```bash
curl http://localhost:31420/health
# ok
```

### Por que `--privileged`

O coast-service gerencia contêineres Docker-in-Docker. A flag `--privileged` concede ao contêiner as capacidades necessárias para executar daemons Docker aninhados.

### Por que bind mount, e não volume Docker

O daemon faz rsync dos arquivos do workspace do seu laptop para o host remoto via SSH. Esses arquivos chegam ao sistema de arquivos do host em `/data/workspaces/{project}/{instance}/`. Se `/data` fosse um volume Docker nomeado, os arquivos ficariam isolados dentro do armazenamento do Docker e invisíveis ao coast-service em execução dentro do contêiner.

Use `-v /data:/data` (bind mount), não `-v coast-data:/data` (volume nomeado).

## Registrar o Remoto

Na sua máquina local:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
```

Com uma porta SSH personalizada:

```bash
coast remote add my-vm ubuntu@10.0.0.1:2222 --key ~/.ssh/coast_key
```

Teste a conectividade:

```bash
coast remote test my-vm
```

Isso verifica o acesso SSH, checa se o coast-service está acessível na porta 31420 pelo túnel SSH e informa a arquitetura do remoto e a versão do coast-service.

## Construir e Executar

```bash
# Build on the remote (uses the remote's native architecture)
coast build --type remote

# Run a remote coast instance
coast run dev-1 --type remote
```

Depois disso, todos os comandos padrão funcionam:

```bash
coast ps dev-1                              # service status
coast exec dev-1 -- bash                    # shell into remote DinD
coast logs dev-1                            # stream service logs
coast assign dev-1 --worktree feature/x     # switch worktree
coast checkout dev-1                        # canonical ports → dev-1
coast ports dev-1                           # show port mappings
```

## Múltiplos Remotos

Você pode registrar mais de uma máquina remota:

```bash
coast remote add dev-server ubuntu@10.0.0.1 --key ~/.ssh/key1
coast remote add gpu-box   ubuntu@10.0.0.2 --key ~/.ssh/key2
coast remote ls
```

Ao executar ou construir, especifique qual remoto deve ser usado como destino:

```bash
coast build --type remote --remote gpu-box
coast run dev-1 --type remote --remote gpu-box
```

Se apenas um remoto estiver registrado, ele será selecionado automaticamente.

## Configuração de Desenvolvimento Local

Para desenvolver o próprio coast-service, use o contêiner de desenvolvimento, que inclui DinD, sshd e hot reload com cargo-watch:

```bash
make coast-service-dev
```

Em seguida, registre o contêiner de desenvolvimento como um remoto:

```bash
coast remote add dev-vm root@localhost:2222 --key $(pwd)/.dev/ssh/coast_dev_key
coast remote test dev-vm
```

Use caminhos absolutos para a flag `--key`.
