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

Si se omite, el contenedor de Coast se inicia sin ejecutar `docker compose up`. Puedes usar [servicios bare](SERVICES.md) o interactuar con el contenedor directamente mediante `coast exec`.

No puedes establecer tanto `compose` como `[services]` en el mismo Coastfile.

### `runtime`

Qué runtime de contenedores usar. Por defecto es `"dind"` (Docker-in-Docker).

- `"dind"` — Docker-in-Docker con `--privileged`. El único runtime probado en producción. Consulta [Runtimes and Services](../concepts_and_terminology/RUNTIMES_AND_SERVICES.md).
- `"sysbox"` — Usa el runtime Sysbox en lugar del modo privilegiado. Requiere que Sysbox esté instalado.
- `"podman"` — Usa Podman como runtime interno de contenedores.

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

Directorio donde se crean los worktrees de git para instancias de Coast. Por defecto es `".coasts"`. Las rutas relativas se resuelven contra la raíz del proyecto.

```toml
[coast]
name = "my-app"
worktree_dir = ".worktrees"
```

Si el directorio es relativo y está dentro del proyecto, Coast lo añade automáticamente a `.gitignore`.

### `autostart`

Si se debe ejecutar automáticamente `docker compose up` (o iniciar servicios bare) cuando se crea una instancia de Coast con `coast run`. Por defecto es `true`.

Establécelo en `false` cuando quieres que el contenedor esté ejecutándose pero quieres iniciar los servicios manualmente — útil para variantes de ejecutores de pruebas donde invocas las pruebas bajo demanda.

```toml
[coast]
name = "my-app"
extends = "Coastfile"
autostart = false
```

### `primary_port`

Nombra un puerto de la sección `[ports]` para usarlo en enlaces rápidos y en el enrutamiento por subdominio. El valor debe coincidir con una clave definida en `[ports]`.

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

Consulta [Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md) para ver cómo esto habilita el enrutamiento por subdominio y las plantillas de URL.

## `[coast.setup]`

Personaliza el propio contenedor de Coast — instalando herramientas, ejecutando pasos de compilación y materializando archivos de configuración. Todo en `[coast.setup]` se ejecuta dentro del contenedor DinD (no dentro de tus servicios de compose).

### `packages`

Paquetes APK para instalar. Estos son paquetes de Alpine Linux, ya que la imagen base de DinD está basada en Alpine.

```toml
[coast.setup]
packages = ["nodejs", "npm", "git", "curl"]
```

### `run`

Comandos de shell ejecutados en orden durante la compilación. Úsalos para instalar herramientas que no están disponibles como paquetes APK.

```toml
[coast.setup]
packages = ["nodejs", "npm", "python3", "wget", "bash", "ca-certificates"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]
```

### `[[coast.setup.files]]`

Archivos a crear dentro del contenedor. Cada entrada tiene un `path` (obligatorio, debe ser absoluto), `content` (obligatorio) y un `mode` opcional (cadena octal de 3-4 dígitos).

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

Reglas de validación para las entradas de archivos:

- `path` debe ser absoluto (empezar con `/`)
- `path` no debe contener componentes `..`
- `path` no debe terminar con `/`
- `mode` debe ser una cadena octal de 3 o 4 dígitos (p. ej., `"600"`, `"0644"`)

## Ejemplo completo

Un contenedor de Coast configurado para desarrollo con Go y Node.js:

```toml
[coast]
name = "my-fullstack-app"
compose = "./docker-compose.yml"
runtime = "dind"
worktree_dir = ".worktrees"
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
