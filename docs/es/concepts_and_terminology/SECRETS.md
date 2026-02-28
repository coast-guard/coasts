# Secretos y Extractores

Los secretos son valores extraídos de tu máquina anfitriona e inyectados en contenedores de Coast como variables de entorno o archivos. Coast extrae secretos en tiempo de compilación, los cifra en reposo en un almacén de claves local y los inyecta cuando se crea una instancia de Coast.

## Tipos de Inyección

Cada secreto tiene un destino `inject` que controla cómo se entrega dentro del contenedor de Coast:

- `env:VAR_NAME` — se inyecta como una variable de entorno.
- `file:/path/in/container` — se monta como un archivo dentro del contenedor.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"

[secrets.credentials]
extractor = "file"
path = "~/.config/my-app/credentials.json"
inject = "file:/run/secrets/credentials.json"
```

## Extractores Integrados

### env

Lee una variable de entorno del host. Este es el extractor más común y simple. Si ya tienes secretos como variables de entorno en tu host — de archivos `.env`, `direnv`, perfiles del shell, o cualquier otra fuente — simplemente reenvíalos a Coast.

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DATABASE_PASSWORD"
```

La mayoría de los proyectos pueden arreglárselas únicamente con el extractor `env`.

### file

Lee un archivo del sistema de archivos del host. Soporta la expansión de `~` para rutas del directorio home. Es bueno para claves SSH, certificados TLS y archivos JSON de credenciales.

```toml
[secrets.ssh_key]
extractor = "file"
path = "~/.ssh/id_ed25519"
inject = "file:/run/secrets/ssh_key"
```

### command

Ejecuta un comando de shell y captura stdout como el valor del secreto. El comando se ejecuta mediante `sh -c`, así que funcionan tuberías, redirecciones y expansión de variables. Esto es útil para obtener secretos desde 1Password CLI, HashiCorp Vault o cualquier fuente dinámica.

```toml
[secrets.op_token]
extractor = "command"
run = "op read 'op://vault/db/password'"
inject = "env:DATABASE_PASSWORD"
```

También puedes usar `command` para transformar o extraer campos específicos de archivos de configuración locales:

```toml
[secrets.claude_config]
extractor = "command"
run = 'python3 -c "import json; print(json.dumps({\"key\": \"value\"}))"'
inject = "file:/root/.claude.json"
```

### keychain

Alias de `macos-keychain`. Lee un elemento de contraseña genérica del Llavero de macOS.

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

El extractor de llavero a menudo es innecesario. Si puedes obtener el mismo valor mediante una variable de entorno o un archivo, prefiere esos enfoques más simples. La extracción desde el llavero es útil cuando el secreto solo existe en el Llavero de macOS y no se exporta fácilmente — por ejemplo, credenciales específicas de una aplicación almacenadas por herramientas de terceros que escriben directamente en el Llavero.

El parámetro `account` es opcional y por defecto es tu nombre de usuario de macOS.

Este extractor solo está disponible en macOS. Referenciarlo en otras plataformas produce un error claro en tiempo de compilación.

## Extractores Personalizados

Si ninguno de los extractores integrados se ajusta a tu flujo de trabajo, Coast recurre a buscar un ejecutable llamado `coast-extractor-{name}` en tu PATH. El ejecutable recibe los parámetros del extractor como JSON por stdin y debe escribir el valor del secreto en stdout.

```toml
[secrets.vault_token]
extractor = "vault"
path = "secret/data/token"
inject = "env:VAULT_TOKEN"
```

Coast invocará `coast-extractor-vault`, pasando `{"path": "secret/data/token"}` por stdin. El código de salida 0 significa éxito; un valor distinto de cero significa fallo (stderr se incluye en el mensaje de error).

## Inyección No Secreta

La sección `[inject]` reenvía variables de entorno y archivos del host hacia Coast sin tratarlos como secretos. Estos valores no se cifran — se pasan directamente.

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.gitconfig", "~/.npmrc"]
```

Usa `[inject]` para configuración que no sea sensible. Usa `[secrets]` para cualquier cosa que deba cifrarse en reposo.

## Los Secretos No se Almacenan en la Compilación

Los secretos se extraen en tiempo de compilación pero no se incrustan en el artefacto de compilación de Coast. Se inyectan cuando se crea una instancia de Coast con `coast run`. Esto significa que puedes compartir artefactos de compilación sin exponer secretos.

Los secretos pueden reinyectarse en tiempo de ejecución sin recompilar. En la interfaz de [Coastguard](COASTGUARD.md), usa la acción **Re-run Secrets** en la pestaña Secrets. Desde la CLI, usa [`coast build --refresh`](BUILDS.md) para reextraer y actualizar secretos.

## TTL y Re-extracción

Los secretos pueden tener un campo opcional `ttl` (tiempo de vida). Cuando un secreto expira, `coast build --refresh` lo reextraerá desde la fuente.

```toml
[secrets.short_lived_token]
extractor = "command"
run = "generate-token --ttl 1h"
inject = "env:AUTH_TOKEN"
ttl = "1h"
```

## Cifrado en Reposo

Todos los secretos extraídos se cifran con AES-256-GCM en un almacén de claves local. La clave de cifrado se almacena en el Llavero de macOS en macOS, o en un archivo con permisos 0600 en Linux.
