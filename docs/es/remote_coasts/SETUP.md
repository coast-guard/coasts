# Configuración

Esta página cubre todo lo necesario para poner en marcha un coast remoto: preparar el host remoto, desplegar coast-service, registrar el remoto y ejecutar tu primer coast remoto.

## Requisitos del host

| Requisito | Por qué |
|---|---|
| Docker | Ejecuta los contenedores de coast-service y DinD |
| `GatewayPorts clientspecified` in sshd_config | Permite que los túneles SSH reversos para [servicios compartidos](../concepts_and_terminology/SHARED_SERVICES.md) se enlacen en todas las interfaces, no solo en localhost |
| Passwordless sudo for SSH user | El daemon usa `sudo rsync` y `sudo chown` para la gestión de archivos del workspace (los directorios remotos del workspace pueden ser propiedad de root debido a operaciones de coast-service) |
| Bind mount for `/data` (not a Docker volume) | El daemon hace rsync de archivos al sistema de archivos del host mediante SSH. Los volúmenes Docker con nombre están aislados del sistema de archivos del host y son invisibles para rsync |
| 50 GB+ disk | Las imágenes Docker existen en el Docker del host, en tarballs y cargadas dentro de contenedores DinD. Consulta [gestión de disco](CLI.md#disk-management) para más detalles |
| SSH access | El daemon se conecta al remoto mediante SSH para túneles, rsync y acceso a la API de coast-service |

## Preparar el host remoto

En una máquina Linux nueva (EC2, GCP, bare metal):

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

Cierra sesión y vuelve a iniciarla para que el cambio del grupo docker surta efecto.

## Desplegar coast-service

Clona el repositorio y construye la imagen de producción:

```bash
git clone https://github.com/coast-guard/coasts.git
cd coasts && git checkout <branch>
docker build -t coast-service -f Dockerfile.coast-service .
```

Ejecútalo con un bind mount (no un volumen Docker):

```bash
docker run -d \
  --name coast-service \
  --privileged \
  -p 31420:31420 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v /data:/data \
  coast-service
```

Verifica que esté en ejecución:

```bash
curl http://localhost:31420/health
# ok
```

### Por qué `--privileged`

coast-service gestiona contenedores Docker-in-Docker. La bandera `--privileged` otorga al contenedor las capacidades necesarias para ejecutar daemons Docker anidados.

### Por qué bind mount y no un volumen Docker

El daemon hace rsync de los archivos del workspace desde tu laptop al host remoto mediante SSH. Esos archivos terminan en el sistema de archivos del host en `/data/workspaces/{project}/{instance}/`. Si `/data` fuera un volumen Docker con nombre, los archivos quedarían aislados dentro del almacenamiento de Docker e invisibles para coast-service ejecutándose dentro del contenedor.

Usa `-v /data:/data` (bind mount), no `-v coast-data:/data` (volumen con nombre).

## Registrar el remoto

En tu máquina local:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
```

Con un puerto SSH personalizado:

```bash
coast remote add my-vm ubuntu@10.0.0.1:2222 --key ~/.ssh/coast_key
```

Probar la conectividad:

```bash
coast remote test my-vm
```

Esto verifica el acceso SSH, comprueba que coast-service sea accesible en el puerto 31420 a través del túnel SSH e informa la arquitectura del remoto y la versión de coast-service.

## Construir y ejecutar

```bash
# Build on the remote (uses the remote's native architecture)
coast build --type remote

# Run a remote coast instance
coast run dev-1 --type remote
```

Después de esto, todos los comandos estándar funcionan:

```bash
coast ps dev-1                              # service status
coast exec dev-1 -- bash                    # shell into remote DinD
coast logs dev-1                            # stream service logs
coast assign dev-1 --worktree feature/x     # switch worktree
coast checkout dev-1                        # canonical ports → dev-1
coast ports dev-1                           # show port mappings
```

## Múltiples remotos

Puedes registrar más de una máquina remota:

```bash
coast remote add dev-server ubuntu@10.0.0.1 --key ~/.ssh/key1
coast remote add gpu-box   ubuntu@10.0.0.2 --key ~/.ssh/key2
coast remote ls
```

Al ejecutar o construir, especifica a qué remoto apuntar:

```bash
coast build --type remote --remote gpu-box
coast run dev-1 --type remote --remote gpu-box
```

Si solo hay un remoto registrado, se selecciona automáticamente.

## Configuración de desarrollo local

Para desarrollar coast-service en sí, usa el contenedor de desarrollo que incluye DinD, sshd y recarga en caliente con cargo-watch:

```bash
make coast-service-dev
```

Luego registra el contenedor de desarrollo como un remoto:

```bash
coast remote add dev-vm root@localhost:2222 --key $(pwd)/.dev/ssh/coast_dev_key
coast remote test dev-vm
```

Usa rutas absolutas para la bandera `--key`.
