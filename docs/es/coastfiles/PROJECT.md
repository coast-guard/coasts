# Proyecto y configuración

La sección `[coast]` es la única sección obligatoria en un Coastfile. Identifica el proyecto y configura cómo se crea el contenedor de Coast. La subsección opcional `[coast.setup]` te permite instalar paquetes y ejecutar comandos dentro del contenedor en tiempo de compilación.

## `[coast]`

### `name` (obligatorio)

Un identificador único para el proyecto. Se usa en nombres de contenedores, nombres de volúmenes, seguimiento de estado y salida de la CLI.

```toml
[coast]
name = "my-app"
```

### `compose`

Ruta a un archivo de Docker Compose. Las rutas relativas se resuelven contra la raíz del proyecto (el directorio que contiene el Coastfile, o `root` si se establece).

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
```

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
```

Si se omite, el contenedor de Coast se inicia sin ejecutar `docker compose up`. Puedes usar [servicios bare](SERVICES.md) o interactuar directamente con el contenedor mediante `coast exec`.

No puedes establecer tanto `compose` como `[services]` en el mismo Coastfile.

### `runtime`

Qué runtime de contenedor usar. Por defecto es `"dind"` (Docker-in-Docker).

- `"dind"` — Docker-in-Docker con `--privileged`. El único runtime probado en producción. Ver [Runtimes and Services](../concepts_and_terminology/RUNTIMES_AND_SERVICES.md).
- `"sysbox"` — Usa el runtime Sysbox en lugar del modo privilegiado. Requiere que Sysbox esté instalado.
- `"podman"` — Usa Podman como el runtime de contenedor interno.

```toml
[coast]
name = "my-app"
runtime = "dind"
```

### `root`

Sobrescribe el directorio raíz del proyecto. Por defecto, la raíz del proyecto es el directorio que contiene el Coastfile. Una ruta relativa se resuelve contra el directorio del Coastfile; una ruta absoluta se usa tal cual.

```toml
[coast]
name = "my-app"
root = "../my-project"
```

Esto es poco común. La mayoría de los proyectos mantienen el Coastfile en la raíz real del proyecto.

### `worktree_dir`

Directorios donde viven los worktrees de git. Acepta una sola cadena o un arreglo de cadenas. Por defecto es `".worktrees"`.

```toml
# Single directory
worktree_dir = ".worktrees"

# Multiple directories, including an external one
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

Las rutas relativas se resuelven contra la raíz del proyecto. Las rutas que comienzan con `~/` o `/` se tratan como directorios **externos** — Coast añade un bind mount separado para que el contenedor pueda acceder a ellos. Así es como te integras con herramientas como Codex que crean worktrees fuera de la raíz del proyecto.

En tiempo de ejecución, Coast detecta automáticamente el directorio de worktree a partir de los worktrees de git existentes (mediante `git worktree list`) y prefiere eso sobre el valor por defecto configurado cuando todos los worktrees coinciden en un único directorio.

Consulta [Worktree Directories](WORKTREE_DIR.md) para la referencia completa, incluido el comportamiento de directorios externos, el filtrado por proyecto y ejemplos.

### `default_worktree_dir`

Qué directorio usar al crear **nuevos** worktrees. Por defecto es la primera entrada en `worktree_dir`. Solo es relevante cuando `worktree_dir` es un arreglo.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
default_worktree_dir = ".worktrees"
```

### `autostart`

Si se debe ejecutar automáticamente `docker compose up` (o iniciar servicios bare) cuando se crea una instancia de Coast con `coast run`. El valor por defecto es `true`.

Establécelo en `false` cuando quieras el contenedor en ejecución pero quieras iniciar los servicios manualmente — útil para variantes de ejecutores de pruebas donde invocas las pruebas bajo demanda.

```toml
[coast]
name = "my-app"
extends = "Coastfile"
autostart = false
```

### `primary_port`

Nombra un puerto de la sección `[ports]` para usarlo en enlaces rápidos y en el enrutamiento por subdominios. El valor debe coincidir con una clave definida en `[ports]`.

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

Consulta [Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md) para ver cómo esto habilita el enrutamiento por subdominios y las plantillas de URL.

## `[coast.setup]`

Personaliza el propio contenedor de Coast — instalando herramientas, ejecutando pasos de compilación y materializando archivos de configuración. Todo en `[coast.setup]` se ejecuta dentro del contenedor DinD (no dentro de tus servicios de compose).

### `packages`

Paquetes APK a instalar. Estos son paquetes de Alpine Linux ya que la imagen base de DinD está basada en Alpine.

```toml
[coast.setup]
packages = ["nodejs", "npm", "git", "curl"]
```

### `run`

Comandos de shell ejecutados en orden durante la compilación. Úsalos para instalar herramientas que no estén disponibles como paquetes APK.

```toml
[coast.setup]
packages = ["nodejs", "npm", "python3", "wget", "bash", "ca-certificates"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]
```

### `[[coast.setup.files]]`

Archivos para crear dentro del contenedor. Cada entrada tiene un `path` (obligatorio, debe ser absoluto), `content` (obligatorio) y un `mode` opcional (cadena octal de 3-4 dígitos).

```toml
[coast.setup]
packages = ["nodejs", "npm"]
run = ["mkdir -p /app/config"]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```

Reglas de validación para entradas de archivos:

- `path` debe ser absoluto (comenzar con `/`)
- `path` no debe contener componentes `..`
- `path` no debe terminar con `/`
- `mode` debe ser una cadena octal de 3 o 4 dígitos (p. ej. `"600"`, `"0644"`)

## Ejemplo completo

Un contenedor de Coast configurado para desarrollo con Go y Node.js:

```toml
[coast]
name = "my-fullstack-app"
compose = "./docker-compose.yml"
runtime = "dind"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
primary_port = "web"

[coast.setup]
packages = ["nodejs", "npm", "python3", "make", "curl", "git", "bash", "ca-certificates", "wget", "gcc", "musl-dev"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz && ln -s /usr/local/go/bin/go /usr/local/bin/go",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
    "pip3 install --break-system-packages pgcli",
]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```
