# Servicios Bare

> **Nota:** Los servicios bare se ejecutan directamente dentro del contenedor de Coast como procesos normales — no están contenerizados. Si tus servicios ya están dockerizados, usa `compose` en su lugar. Los servicios bare son más adecuados para configuraciones simples donde quieres evitar el overhead de escribir un Dockerfile y un docker-compose.yml.

Las secciones `[services.*]` definen procesos que Coast ejecuta directamente dentro del contenedor DinD, sin Docker Compose. Esta es una alternativa a usar un archivo `compose` — no puedes usar ambos en el mismo Coastfile.

Los servicios bare son supervisados por Coast con captura de logs y políticas de reinicio opcionales. Para un contexto más profundo sobre cómo funcionan los servicios bare, sus limitaciones y cuándo migrar a compose, consulta [Servicios Bare](../concepts_and_terminology/BARE_SERVICES.md).

## Definir un servicio

Cada servicio es una sección TOML con nombre bajo `[services]`. El campo `command` es obligatorio.

```toml
[services.web]
command = "node server.js"
port = 3000
```

### `command` (obligatorio)

El comando de shell a ejecutar. No debe estar vacío ni contener solo espacios en blanco.

```toml
[services.web]
command = "npx next dev --turbopack --port 3000 --hostname 0.0.0.0"
```

### `port`

El puerto en el que el servicio escucha. Se usa para comprobación de salud e integración de reenvío de puertos. Si se especifica, debe ser distinto de cero.

```toml
[services.web]
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

### `restart`

Política de reinicio si el proceso finaliza. Por defecto es `"no"`.

- `"no"` — no reiniciar
- `"on-failure"` — reiniciar solo si el proceso termina con un código distinto de cero
- `"always"` — reiniciar siempre

```toml
[services.web]
command = "node server.js"
port = 3000
restart = "on-failure"
```

### `install`

Comandos a ejecutar antes de iniciar el servicio (p. ej., instalar dependencias). Acepta una única cadena o un array de cadenas.

```toml
[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

```toml
[services.web]
install = ["npm install", "npm run build"]
command = "npm start"
port = 3000
```

## Exclusión mutua con compose

Un Coastfile no puede definir tanto `compose` como `[services]`. Si tienes un campo `compose` en `[coast]`, agregar cualquier sección `[services.*]` es un error. Elige un enfoque por Coastfile.

Si necesitas que algunos servicios estén contenerizados mediante compose y otros se ejecuten como bare, usa compose para todos — consulta [la guía de migración en Servicios Bare](../concepts_and_terminology/BARE_SERVICES.md) para saber cómo pasar de servicios bare a compose.

## Ejemplos

### Aplicación Next.js de un solo servicio

```toml
[coast]
name = "my-frontend"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --turbopack --port 3002 --hostname 0.0.0.0"
port = 3002
restart = "on-failure"

[ports]
web = 3002
```

### Servidor web con worker en segundo plano

```toml
[coast]
name = "my-app"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "node server.js"
port = 3000
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
web = 3000
```

### Servicio Python con instalación de varios pasos

```toml
[coast]
name = "ml-service"

[coast.setup]
packages = ["python3", "py3-pip"]

[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
restart = "on-failure"

[ports]
api = 8000
```
