# Costas remotas

> **Beta.** Las costas remotas son completamente funcionales, pero las banderas del CLI, el esquema de Coastfile y la API de coast-service pueden cambiar en futuras versiones. Si descubre un error o defecto, por favor abra un pull request o registre un issue.

Las costas remotas ejecutan sus servicios en una máquina remota mientras mantienen la experiencia de desarrollo idéntica a la de las costas locales. `coast run`, `coast assign`, `coast exec`, `coast ps`, `coast logs` y todos los demás comandos funcionan de la misma manera. El daemon detecta que la instancia es remota y enruta las operaciones de forma transparente a través de un túnel SSH.

## Por qué remoto

Las costas locales ejecutan todo en su laptop. Cada instancia de costa ejecuta un contenedor completo de Docker-in-Docker con toda su pila de compose: servidor web, API, workers, bases de datos, cachés, servidor de correo. Eso funciona hasta que su laptop se queda sin RAM o espacio en disco.

Un proyecto full-stack con varios servicios puede consumir una cantidad significativa de RAM por costa. Ejecute unas pocas costas en paralelo y alcanzará el límite de su laptop.

```text
  coast-1         coast-2         coast-3         coast-4
  ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
  │ worker   │   │ worker   │   │ worker   │   │ worker   │
  │ api      │   │ api      │   │ api      │   │ api      │
  │ admin    │   │ admin    │   │ admin    │   │ admin    │
  │ web      │   │ web      │   │ web      │   │ web      │
  │ mailhog  │   │ mailhog  │   │ mailhog  │   │ mailhog  │
  │          │   │          │   │          │   │          │
  │ 12 GB    │   │ 12 GB    │   │ 12 GB    │   │ 12 GB    │
  └──────────┘   └──────────┘   └──────────┘   └──────────┘

  Total: 48 GB RAM on your laptop
```

Las costas remotas le permiten escalar horizontalmente moviendo algunas de sus costas a máquinas remotas. Los contenedores DinD, los servicios de compose y las compilaciones de imágenes se ejecutan de forma remota, mientras que su editor y agentes permanecen locales. Los servicios compartidos como Postgres y Redis también permanecen locales, manteniendo su base de datos sincronizada entre instancias locales y remotas mediante túneles SSH inversos.

```text
  Your Machine                         Remote Server
  ┌─────────────────────┐             ┌─────────────────────────┐
  │  editor + agents    │             │  coast-1 (all services) │
  │                     │  SSH        │  coast-2 (all services) │
  │  shared services    │──tunnels──▶ │  coast-3 (all services) │
  │  (postgres, redis)  │             │  coast-4 (all services) │
  └─────────────────────┘             └─────────────────────────┘

  Laptop: lightweight                  Server: 64 GB RAM, 16 CPU
```

Escale horizontalmente su entorno de ejecución de localhost.

## Inicio rápido

```bash
# 1. Register a remote machine
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
coast remote test my-vm

# 2. Build on the remote (uses remote's native architecture)
coast build --type remote

# 3. Run a remote coast
coast run dev-1 --type remote

# 4. Everything works as usual
coast ps dev-1
coast exec dev-1 -- bash
coast assign dev-1 --worktree feature/x
coast checkout dev-1
```

Para obtener instrucciones completas de configuración, incluida la preparación del host y el despliegue de coast-service, consulte [Setup](SETUP.md).

## Referencia

| Página | Qué cubre |
|------|----------------|
| [Architecture](ARCHITECTURE.md) | La división en dos contenedores (shell coast + remote coast), la capa de túnel SSH, la cadena de reenvío de puertos y cómo el daemon enruta las solicitudes |
| [Setup](SETUP.md) | Requisitos del host, despliegue de coast-service, registro de remotos e inicio rápido de extremo a extremo |
| [File Sync](FILE_SYNC.md) | rsync para transferencia masiva, mutagen para sincronización continua, ciclo de vida a través de run/assign/stop, exclusiones y manejo de condiciones de carrera |
| [Builds](BUILDS.md) | Compilación en el remoto para arquitectura nativa, transferencia de artefactos, el symlink `latest-remote`, reutilización de arquitectura y poda automática |
| [CLI and Configuration](CLI.md) | Comandos `coast remote`, configuración de `Coastfile.remote`, gestión de disco y `coast remote prune` |

## Véase también

- [Remotes](../concepts_and_terminology/REMOTES.md) -- resumen conceptual en el glosario de terminología
- [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) -- cómo los servicios compartidos locales se tunelizan de forma inversa hacia las costas remotas
- [Ports](../concepts_and_terminology/PORTS.md) -- cómo la capa de túnel SSH encaja en el modelo de puertos canónicos/dinámicos
