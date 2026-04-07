# Настройка

На этой странице описано всё, что нужно для запуска удалённого coast: подготовка удалённого хоста, развёртывание coast-service, регистрация удалённого узла и запуск вашего первого удалённого coast.

## Требования к хосту

| Требование | Зачем |
|---|---|
| Docker | Запускает контейнеры coast-service и DinD |
| `GatewayPorts clientspecified` в sshd_config | Позволяет обратным SSH-туннелям для [shared services](../concepts_and_terminology/SHARED_SERVICES.md) привязываться ко всем интерфейсам, а не только к localhost |
| Passwordless sudo для SSH-пользователя | Демон использует `sudo rsync` и `sudo chown` для управления файлами рабочего пространства (удалённые директории рабочего пространства могут принадлежать root после операций coast-service) |
| Bind mount для `/data` (не Docker volume) | Демон синхронизирует файлы на файловую систему хоста через SSH с помощью rsync. Именованные Docker volumes изолированы от файловой системы хоста и недоступны для rsync |
| Диск 50 ГБ+ | Docker-образы существуют в Docker хоста, в tar-архивах и загружаются в контейнеры DinD. Подробности см. в разделе [управление диском](CLI.md#disk-management) |
| Доступ по SSH | Демон подключается к удалённому узлу по SSH для туннелей, rsync и доступа к API coast-service |

## Подготовьте удалённый хост

На чистой Linux-машине (EC2, GCP, bare metal):

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

Выйдите из системы и войдите снова, чтобы изменение группы docker вступило в силу.

## Развёртывание coast-service

Клонируйте репозиторий и соберите production-образ:

```bash
git clone https://github.com/coast-guard/coasts.git
cd coasts && git checkout <branch>
docker build -t coast-service -f Dockerfile.coast-service .
```

Запустите его с bind mount (не Docker volume):

```bash
docker run -d \
  --name coast-service \
  --privileged \
  -p 31420:31420 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v /data:/data \
  coast-service
```

Убедитесь, что он запущен:

```bash
curl http://localhost:31420/health
# ok
```

### Почему `--privileged`

coast-service управляет контейнерами Docker-in-Docker. Флаг `--privileged` предоставляет контейнеру возможности, необходимые для запуска вложенных демонов Docker.

### Почему bind mount, а не Docker volume

Демон синхронизирует файлы рабочего пространства с вашего ноутбука на удалённый хост по SSH с помощью rsync. Эти файлы попадают в файловую систему хоста по пути `/data/workspaces/{project}/{instance}/`. Если бы `/data` был именованным Docker volume, файлы были бы изолированы внутри хранилища Docker и недоступны для coast-service, работающего внутри контейнера.

Используйте `-v /data:/data` (bind mount), а не `-v coast-data:/data` (named volume).

## Регистрация удалённого узла

На вашей локальной машине:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
```

С пользовательским SSH-портом:

```bash
coast remote add my-vm ubuntu@10.0.0.1:2222 --key ~/.ssh/coast_key
```

Проверьте подключение:

```bash
coast remote test my-vm
```

Это проверяет доступ по SSH, убеждается, что coast-service доступен на порту 31420 через SSH-туннель, и сообщает архитектуру удалённого узла и версию coast-service.

## Сборка и запуск

```bash
# Build on the remote (uses the remote's native architecture)
coast build --type remote

# Run a remote coast instance
coast run dev-1 --type remote
```

После этого работают все стандартные команды:

```bash
coast ps dev-1                              # service status
coast exec dev-1 -- bash                    # shell into remote DinD
coast logs dev-1                            # stream service logs
coast assign dev-1 --worktree feature/x     # switch worktree
coast checkout dev-1                        # canonical ports → dev-1
coast ports dev-1                           # show port mappings
```

## Несколько удалённых узлов

Вы можете зарегистрировать более одной удалённой машины:

```bash
coast remote add dev-server ubuntu@10.0.0.1 --key ~/.ssh/key1
coast remote add gpu-box   ubuntu@10.0.0.2 --key ~/.ssh/key2
coast remote ls
```

При запуске или сборке укажите, какой удалённый узел использовать:

```bash
coast build --type remote --remote gpu-box
coast run dev-1 --type remote --remote gpu-box
```

Если зарегистрирован только один удалённый узел, он выбирается автоматически.

## Локальная dev-настройка

Для разработки самого coast-service используйте dev-контейнер, который включает DinD, sshd и горячую перезагрузку cargo-watch:

```bash
make coast-service-dev
```

Затем зарегистрируйте dev-контейнер как удалённый узел:

```bash
coast remote add dev-vm root@localhost:2222 --key $(pwd)/.dev/ssh/coast_dev_key
coast remote test dev-vm
```

Используйте абсолютные пути для флага `--key`.
