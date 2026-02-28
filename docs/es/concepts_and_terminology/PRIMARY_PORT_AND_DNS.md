# Puerto principal y DNS

El puerto principal es una función opcional de conveniencia que crea un enlace rápido a uno de tus servicios — normalmente tu frontend web. Aparece como una insignia clicable en Coastguard y una entrada con estrella en `coast ports`. No cambia cómo funcionan los puertos; solo elige uno para destacarlo.

## Configurar el puerto principal

Añade `primary_port` a la sección `[coast]` de tu Coastfile, haciendo referencia a una clave de [`[ports]`](PORTS.md):

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
backend = 8080
```

Si tu proyecto solo tiene un puerto, Coast lo detecta automáticamente como el principal — no necesitas configurarlo explícitamente.

También puedes alternar el principal desde la pestaña Ports de Coastguard haciendo clic en el icono de estrella junto a cualquier servicio, o desde la CLI con `coast ports set-primary`. La configuración es por build, por lo que todas las instancias creadas a partir del mismo build comparten el mismo principal.

## Lo que habilita

```text
coast ports dev-1

  SERVICE    CANONICAL  DYNAMIC
  ★ web      3000       62217
    backend  8080       63889
```

El servicio con estrella es tu principal. En Coastguard, aparece como una insignia clicable junto al nombre de la instancia — con un clic se abre tu app en el navegador.

Esto es especialmente útil para:

- **Agentes del lado del host** — dale a tu agente de IA una sola URL contra la que comprobar cambios. En lugar de decirle "abre localhost:62217", la URL del puerto principal está disponible programáticamente desde `coast ls` y la API del daemon.
- **MCPs de navegador** — si tu agente usa un MCP de navegador para verificar cambios de UI, la URL del puerto principal es el objetivo canónico al que apuntarlo.
- **Iteración rápida** — acceso con un clic al servicio que miras con más frecuencia.

El puerto principal es totalmente opcional. Todo funciona sin él — es una función de calidad de vida para una navegación más rápida.

## Enrutamiento por subdominios

Cuando ejecutas múltiples instancias de Coast con bases de datos aisladas, todas comparten `localhost` en el navegador. Esto significa que las cookies establecidas por `localhost:62217` (dev-1) son visibles para `localhost:63104` (dev-2). Si tu app usa cookies de sesión, iniciar sesión en una instancia puede interferir con otra.

El enrutamiento por subdominios soluciona esto dando a cada instancia su propio origen:

```text
Without subdomain routing:
  dev-1 web  →  http://localhost:62217
  dev-2 web  →  http://localhost:63104
  (cookies shared — both are "localhost")

With subdomain routing:
  dev-1 web  →  http://dev-1.localhost:62217
  dev-2 web  →  http://dev-2.localhost:63104
  (cookies isolated — different subdomains)
```

Habilítalo por proyecto desde la pestaña Ports de Coastguard (toggle en la parte inferior de la página) o mediante la API de configuración del daemon.

### Compensación: CORS

La desventaja es que tu aplicación puede necesitar ajustes de CORS. Si tu frontend en `dev-1.localhost:3000` hace solicitudes de API a `dev-1.localhost:8080`, el navegador las trata como de origen cruzado porque el puerto difiere. La mayoría de los servidores de desarrollo ya manejan esto, pero si ves errores de CORS tras habilitar el enrutamiento por subdominios, revisa la configuración de orígenes permitidos de tu aplicación.

## Plantillas de URL

Cada servicio tiene una plantilla de URL que controla cómo se generan sus enlaces. El valor predeterminado es:

```text
http://localhost:<port>
```

El marcador `<port>` se sustituye por el número de puerto real — el puerto canónico cuando la instancia está [checked out](CHECKOUT.md), o el puerto dinámico en caso contrario. Cuando el enrutamiento por subdominios está habilitado, `localhost:` se sustituye por `{instance}.localhost:`.

Puedes personalizar las plantillas por servicio desde la pestaña Ports de Coastguard (icono de lápiz junto a cada servicio). Esto es útil si tu servidor de desarrollo usa HTTPS, un hostname personalizado o un esquema de URL no estándar:

```text
https://my-service.localhost:<port>
```

Las plantillas se almacenan en la configuración del daemon y persisten entre reinicios.

## Configuración de DNS

La mayoría de los navegadores resuelven `*.localhost` a `127.0.0.1` de fábrica, por lo que el enrutamiento por subdominios funciona sin ninguna configuración de DNS.

Si necesitas resolución de dominio personalizada (p. ej. `*.localcoast`), Coast incluye un servidor DNS incrustado. Configúralo una vez:

```bash
coast dns setup    # writes /etc/resolver/localcoast (requires sudo)
coast dns status   # check if DNS is configured
coast dns remove   # remove the resolver entry
```

Esto es opcional y solo es necesario si `*.localhost` no funciona en tu navegador o si quieres un TLD personalizado.
