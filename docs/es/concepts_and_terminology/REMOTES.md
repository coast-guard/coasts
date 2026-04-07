# Remotos

Un coast remoto ejecuta tus servicios en una máquina remota en lugar de en tu laptop. La experiencia de la CLI y la UI es idéntica a la de los coasts locales -- `coast run`, `coast assign`, `coast exec`, `coast ps` y `coast checkout` funcionan todos de la misma manera. El daemon detecta que la instancia es remota y enruta las operaciones a través de un túnel SSH hacia `coast-service` en el host remoto.

## Local vs Remote

| | Coast local | Coast remoto |
|---|---|---|
| Contenedor DinD | Se ejecuta en tu máquina | Se ejecuta en la máquina remota |
| Servicios Compose | Dentro del DinD local | Dentro del DinD remoto |
| Edición de archivos | Montaje bind directo | Shell coast (local) + sincronización rsync/mutagen |
| Acceso a puertos | Reenviador `socat` | Túnel SSH `-L` + reenviador `socat` |
| Servicios compartidos | Red bridge | Túnel inverso SSH `-R` |
| Arquitectura de compilación | La arquitectura de tu máquina | La arquitectura de la máquina remota |

## Cómo funciona

Cada coast remoto crea dos contenedores:

1. Un **shell coast** en tu máquina local. Este es un contenedor Docker liviano (`sleep infinity`) con los mismos montajes bind que un coast normal (`/host-project`, `/workspace`). Existe para que los agentes del host puedan editar archivos que se sincronizan con el remoto.

2. Un **coast remoto** en la máquina remota, gestionado por `coast-service`. Este ejecuta el contenedor DinD real con tus servicios compose, usando puertos dinámicos.

El daemon los conecta mediante túneles SSH:

- **Túneles forward** (`ssh -L`): asignan cada puerto dinámico local al puerto dinámico remoto correspondiente, de modo que `localhost:{dynamic}` llegue al servicio remoto.
- **Túneles reverse** (`ssh -R`): exponen los [servicios compartidos](SHARED_SERVICES.md) locales (Postgres, Redis) al contenedor DinD remoto.

## Registro de remotos

Los remotos se registran con el daemon y se almacenan en `state.db`:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/coast_key
coast remote test my-vm
coast remote ls
coast remote rm my-vm
```

Los detalles de conexión (host, usuario, puerto, clave SSH) viven en la base de datos del daemon, no en tu Coastfile. El Coastfile solo declara las preferencias de sincronización mediante la sección `[remote]`.

## Compilaciones remotas

Las compilaciones ocurren en la máquina remota para que las imágenes usen la arquitectura nativa del remoto. Una Mac ARM puede compilar imágenes x86_64 en un remoto x86_64 sin compilación cruzada.

Después de compilar, el artefacto se transfiere de vuelta a tu máquina local para su reutilización. Si otro remoto tiene la misma arquitectura, el artefacto precompilado puede desplegarse directamente sin volver a compilar. Consulta [Builds](BUILDS.md) para más información sobre cómo se estructuran los artefactos de compilación.

## Sincronización de archivos

Los coasts remotos usan rsync para la transferencia masiva inicial y mutagen para la sincronización continua en tiempo real. Ambas herramientas se ejecutan dentro de contenedores coast (el shell coast y la imagen coast-service), no en tu máquina host. Consulta la guía de [Remote Coasts](../remote_coasts/README.md) para obtener detalles sobre la configuración de sincronización.

## Gestión de disco

Las máquinas remotas acumulan volúmenes Docker, directorios de workspace y archivos tar de imágenes. Cuando `coast rm` elimina una instancia remota, todos los recursos asociados se limpian. Para recursos huérfanos de operaciones fallidas, usa `coast remote prune`.

## Configuración

Para instrucciones completas de configuración, incluidos los requisitos del host, el despliegue de coast-service y la configuración de Coastfile, consulta la guía de [Remote Coasts](../remote_coasts/README.md).
