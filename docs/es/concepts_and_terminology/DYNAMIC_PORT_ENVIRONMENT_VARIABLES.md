# Variables de entorno de puertos dinámicos

Cada instancia de Coast recibe un conjunto de variables de entorno que exponen el [puerto dinámico](PORTS.md) asignado a cada servicio. Estas variables están disponibles tanto dentro de servicios bare como de contenedores compose, y permiten que tu aplicación descubra en tiempo de ejecución su puerto accesible externamente.

## Convención de nombres

Coast deriva el nombre de la variable a partir del nombre lógico del servicio en tu sección `[ports]`:

1. Convertir a mayúsculas
2. Reemplazar los caracteres no alfanuméricos por guiones bajos
3. Añadir `_DYNAMIC_PORT`

```text
[ports] key          Environment variable
─────────────        ────────────────────────────
web             →    WEB_DYNAMIC_PORT
postgres        →    POSTGRES_DYNAMIC_PORT
backend-test    →    BACKEND_TEST_DYNAMIC_PORT
svc.v2          →    SVC_V2_DYNAMIC_PORT
```

Si el nombre del servicio comienza con un dígito, Coast antepone un guion bajo a la variable (por ejemplo, `9svc` se convierte en `_9SVC_DYNAMIC_PORT`). Un nombre vacío recurre a `SERVICE_DYNAMIC_PORT`.

## Ejemplo

Dado este Coastfile:

```toml
[ports]
web = 3000
api = 8080
postgres = 5432
```

Cada instancia de Coast creada a partir de esta compilación tendrá tres variables de entorno adicionales:

```text
WEB_DYNAMIC_PORT=62217
API_DYNAMIC_PORT=55681
POSTGRES_DYNAMIC_PORT=56905
```

Los números de puerto reales se asignan en el momento de `coast run` y difieren según la instancia.

## Cuándo usarlas

El caso de uso más común es configurar servicios que incrustan su propia URL en las respuestas: callbacks de autenticación, URI de redirección de OAuth, orígenes de CORS o URL de webhooks. Estos servicios necesitan conocer el puerto que usan los clientes externos, no el puerto interno en el que escuchan.

Por ejemplo, una aplicación Next.js que usa NextAuth necesita que `AUTH_URL` esté configurado con la dirección accesible externamente. Dentro de Coast, Next.js siempre escucha en el puerto 3000, pero el puerto del host es dinámico:

```toml
[services.web]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} yarn dev:web"
port = 3000
```

El valor de reserva `:-3000` significa que el comando también funciona fuera de Coast, donde `WEB_DYNAMIC_PORT` no está configurada.

## Precedencia

Si ya existe una variable de entorno con el mismo nombre en el contenedor de Coast (establecida mediante secrets, inject o el entorno de compose), Coast no la sobrescribe. El valor existente tiene precedencia.

## Disponibilidad

Las variables de puerto dinámico se inyectan en el entorno del contenedor de Coast al iniciarse. Están disponibles para:

- Comandos `install` de servicios bare
- Procesos `command` de servicios bare
- Contenedores de servicios compose (a través del entorno del contenedor)
- Comandos ejecutados mediante `coast exec`

Los valores no cambian durante la vida útil de la instancia. Si detienes e inicias la instancia, conserva los mismos puertos dinámicos.

## Ver también

- [Ports](PORTS.md) - puertos canónicos frente a dinámicos y cómo checkout alterna entre ellos
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - enrutamiento por subdominios y aislamiento de cookies entre instancias
