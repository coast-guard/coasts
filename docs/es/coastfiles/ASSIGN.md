# Asignar

La sección `[assign]` controla lo que sucede con los servicios dentro de una instancia de Coast cuando cambias de rama con `coast assign`. Cada servicio puede configurarse con una estrategia diferente según si necesita una reconstrucción completa, un reinicio, una recarga en caliente o nada en absoluto.

Para saber cómo funcionan `coast assign` y `coast unassign` en tiempo de ejecución, consulta [Assign](../concepts_and_terminology/ASSIGN.md).

## `[assign]`

### `default`

La acción predeterminada aplicada a todos los servicios al cambiar de rama. El valor predeterminado es `"restart"` si se omite por completo toda la sección `[assign]`.

- **`"none"`** — no hacer nada. El servicio sigue ejecutándose tal cual. Útil para bases de datos y cachés que no dependen del código.
- **`"hot"`** — el código ya está montado en vivo mediante el [filesystem](../concepts_and_terminology/FILESYSTEM.md), por lo que el servicio incorpora los cambios automáticamente (p. ej., mediante un observador de archivos o recarga en caliente). No se necesita reiniciar el contenedor.
- **`"restart"`** — reiniciar el contenedor del servicio. Úsalo cuando el servicio lee el código al inicio pero no necesita una reconstrucción completa de la imagen.
- **`"rebuild"`** — reconstruir la imagen de Docker del servicio y reiniciar. Es necesario cuando el código está integrado en la imagen mediante `COPY` o `ADD` en el Dockerfile.

```toml
[assign]
default = "none"
```

### `[assign.services]`

Anulaciones por servicio. Cada clave es un nombre de servicio de compose y el valor es una de las cuatro acciones anteriores.

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

Esto te permite dejar bases de datos y cachés sin cambios (`"none"` mediante el valor predeterminado) mientras reconstruyes o reinicias solo los servicios que dependen del código que cambió.

### `[assign.rebuild_triggers]`

Patrones de archivos que fuerzan una reconstrucción para servicios específicos, incluso si su acción predeterminada es algo más liviano. Cada clave es un nombre de servicio y el valor es una lista de rutas o patrones de archivos.

```toml
[assign]
default = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json", "package-lock.json"]
```

### `exclude_paths`

Una lista de rutas que se excluirán de la sincronización del worktree durante `coast assign`. Es útil en monorepos grandes donde ciertos directorios son irrelevantes para los servicios que se ejecutan en Coast y, de otro modo, ralentizarían la operación de asignación.

```toml
[assign]
default = "none"
exclude_paths = ["apps/ide", "apps/extension", "apps/ide-extension"]

[assign.services]
backend = "hot"
web = "hot"
```

## Ejemplos

### Reconstruir app, dejar todo lo demás intacto

Cuando tu servicio de app integra el código en su imagen de Docker pero tus bases de datos son independientes de los cambios de código:

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

### Recarga en caliente de frontend y backend

Cuando ambos servicios usan observadores de archivos (p. ej., servidor dev de Next.js, Go air, nodemon) y el código está montado en vivo:

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

### Reconstrucción por servicio con triggers

El servicio API normalmente solo se reinicia, pero si `Dockerfile` o `package.json` cambian, se reconstruye:

```toml
[assign]
default = "none"

[assign.services]
api = "restart"
worker = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json"]
```

### Reconstrucción completa para todo

Cuando todos los servicios integran el código en sus imágenes:

```toml
[assign]
default = "rebuild"
```
