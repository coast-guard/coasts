# Primeros pasos con Coasts

Si aún no lo has hecho, completa primero la instalación y los requisitos de abajo. Luego, esta guía explica cómo usar Coast en un proyecto.

## Installing

```bash
curl -fsSL https://coasts.dev/install | sh
coast daemon install
```

*Si decides no ejecutar `coast daemon install`, eres responsable de iniciar el daemon manualmente con `coast daemon start` cada vez.*

## Requirements

- macOS
- Docker Desktop
- Un proyecto usando Git
- Node.js
- `socat` (`brew install socat` en macOS)

```text
Linux note: We have not tested Coasts on Linux yet, but Linux support is planned.
You can try to run Coasts on Linux today, but we do not provide guarantees that it will work correctly.
```

## Configurar Coasts en un proyecto

Agrega un Coastfile en la raíz de tu proyecto. Asegúrate de no estar en un worktree al instalar.

```text
my-project/
├── Coastfile              <-- esto es lo que lee Coast
├── docker-compose.yml
├── Dockerfile
├── src/
│   └── ...
└── ...
```

El `Coastfile` apunta a tus recursos existentes de desarrollo local y añade configuración específica de Coasts — consulta la [documentación de Coastfiles](coastfiles/README.md) para el esquema completo:

```toml
[coast]
name = "my-project"
compose = "./docker-compose.yml"

[ports]
web = 3000
db = 5432
```

Un Coastfile es un archivo TOML ligero que *típicamente* apunta a tu `docker-compose.yml` existente (también funciona con configuraciones de desarrollo local sin contenedores) y describe las modificaciones necesarias para ejecutar tu proyecto en paralelo: asignaciones de puertos, estrategias de volúmenes y secretos. Colócalo en la raíz de tu proyecto.

La forma más rápida de crear un Coastfile para tu proyecto es dejar que tu agente de programación lo haga.

La CLI de Coasts incluye un prompt integrado que enseña a cualquier agente de IA el esquema completo del Coastfile y la CLI. Puedes verlo aquí: [installation_prompt.txt](installation_prompt.txt)

Pásalo directamente a tu agente, o copia el [prompt de instalación](installation_prompt.txt) y pégalo en el chat de tu agente:

```bash-emphasis
# Claude Code
claude -p "$(coast installation-prompt)"

# Codex
codex "$(coast installation-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast installation-prompt)"
```

El prompt cubre el formato TOML del Coastfile, las estrategias de volúmenes, la inyección de secretos y todos los comandos relevantes de la CLI. Tu agente analizará tu proyecto y generará un Coastfile.

## Tu primer Coast

Antes de iniciar tu primer Coast, baja cualquier entorno de desarrollo en ejecución. Si estás usando Docker Compose, ejecuta `docker-compose down`. Si tienes servidores de desarrollo local ejecutándose, detenlos. Coasts gestiona sus propios puertos y entrará en conflicto con cualquier cosa que ya esté escuchando.

Una vez que tu Coastfile esté listo:

```bash
coast build
coast run dev-1
```

Verifica que tu instancia esté en ejecución:

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -             ~/dev/my-project
```

Mira dónde están escuchando tus servicios:

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

Cada instancia obtiene su propio conjunto de puertos dinámicos para que múltiples instancias puedan ejecutarse en paralelo. Para mapear una instancia de vuelta a los puertos canónicos de tu proyecto, haz checkout de ella:

```bash
coast checkout dev-1
```

Esto significa que el runtime ahora está en checkout y los puertos canónicos de tu proyecto (como `3000`, `5432`) se enrutarán a esta instancia de Coast.

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -         ✓   ~/dev/my-project
```

Para abrir la UI de observabilidad de Coastguard para tu proyecto:

```bash
coast ui
```

## ¿Qué sigue?

- Configura una [skill para tu agente host](SKILLS_FOR_HOST_AGENTS.md) para que sepa cómo interactuar con Coasts
