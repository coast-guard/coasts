# Variables

Los Coastfiles admiten la interpolación de variables de entorno en todos los valores de tipo cadena. Las variables se resuelven en tiempo de análisis, antes de que se procese el TOML, por lo que funcionan en cualquier sección y en cualquier posición de valor.

## Sintaxis

Haz referencia a una variable de entorno con `${VAR_NAME}`:

```toml
[coast]
name = "${PROJECT_NAME}"
compose = "${COMPOSE_PATH}"

[ports]
web = ${WEB_PORT}
```

Los nombres de las variables deben comenzar con una letra o un guion bajo, seguidos de letras, dígitos o guiones bajos (coincidiendo con el patrón `[A-Za-z_][A-Za-z0-9_]*`).

## Valores predeterminados

Usa `${VAR:-default}` para proporcionar un valor de respaldo cuando la variable no esté definida:

```toml
[coast]
name = "${PROJECT_NAME:-my-app}"
runtime = "${RUNTIME:-dind}"

[ports]
web = ${WEB_PORT:-3000}
api = ${API_PORT:-8080}
```

Si `PROJECT_NAME` está definida en el entorno, se usa su valor. Si no, se sustituye `my-app`. Los valores predeterminados pueden contener cualquier carácter excepto `}`.

## Variables no definidas

Cuando se hace referencia a una variable sin un valor predeterminado y no está definida en el entorno, Coast **conserva el texto literal `${VAR}`** y emite una advertencia:

```
warning: undefined environment variable 'DB_HOST' preserved as literal '${DB_HOST}'; use '${DB_HOST:-}' for explicit empty, or '$${DB_HOST}' to escape entirely
```

Conservar la referencia (en lugar de reemplazarla silenciosamente por una cadena vacía) permite que comandos de shell como `ARCH=$(uname -m) && curl .../linux-${ARCH}.tar.gz` sigan funcionando: el shell del Dockerfile aún puede expandir `${ARCH}` en tiempo de compilación aunque Coast nunca la haya definido.

Si realmente quieres una sustitución vacía cuando la variable falta, usa el valor predeterminado vacío explícito:

```toml
[coast]
name = "${PROJECT_NAME:-}"   # "" cuando PROJECT_NAME no está definida
```

Si quieres el texto literal `${VAR}` sin ninguna advertencia, escápalo con `$${VAR}` (consulta [Escaping](#escaping) más abajo).

## Escaping

Para producir un `${...}` literal en tu Coastfile (por ejemplo, en un valor que debe contener el texto `${VAR}` en lugar de su valor expandido), duplica el signo de dólar inicial:

```toml
[coast.setup]
run = ["echo '$${NOT_EXPANDED}'"]
```

Esto produce la cadena literal `echo '${NOT_EXPANDED}'` sin intentar buscar la variable.

## Ejemplos

### Secretos con claves obtenidas del entorno

```toml
[secrets.api_key]
extractor = "env"
var = "${API_KEY_ENV_VAR:-MY_API_KEY}"
inject = "env:API_KEY"
```

### Configuración de servicio compartido

```toml
[shared_services.postgres]
image = "postgres:${PG_VERSION:-16}"
env = [
    "POSTGRES_USER=${DB_USER:-coast}",
    "POSTGRES_PASSWORD=${DB_PASSWORD:-dev}",
    "POSTGRES_DB=${DB_NAME:-coast_dev}",
]
ports = [5432]
```

### Ruta de compose por entorno

```toml
[coast]
name = "my-app"
compose = "${COMPOSE_FILE:-./docker-compose.yml}"
```

## Variables vs Secrets

La interpolación de variables y los [secrets](SECRETS.md) tienen propósitos diferentes:

| | Variables (`${VAR}`) | Secrets (`[secrets.*]`) |
|---|---|---|
| **Cuándo se resuelven** | Tiempo de análisis (antes del procesamiento de TOML) | Tiempo de compilación (extraídos de las fuentes configuradas) |
| **Dónde se almacenan** | Integradas en el Coastfile resuelto | Keystore cifrado (`~/.coast/keystore.db`) |
| **Caso de uso** | Configuración que varía según el entorno (puertos, rutas, etiquetas de imagen) | Credenciales sensibles (claves API, tokens, contraseñas) |
| **Visibles en los artefactos** | Sí (los valores aparecen en `coastfile.toml` dentro de la compilación) | No (solo aparecen los nombres de los secretos en el manifiesto) |

Usa variables para configuración no sensible que cambia entre máquinas o entornos de CI. Usa secrets para valores que nunca deben aparecer en los artefactos de compilación.
