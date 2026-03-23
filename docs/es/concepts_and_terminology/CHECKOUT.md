# Checkout

Checkout controla qué instancia de Coast posee los [puertos canónicos](PORTS.md) de tu proyecto. Cuando haces checkout de un Coast, `localhost:3000`, `localhost:5432` y cualquier otro puerto canónico se asigna directamente a esa instancia.

```bash
coast checkout dev-1
```

```text
Before checkout:
  localhost:3000  ──→  (nothing)
  localhost:5432  ──→  (nothing)

After checkout:
  localhost:3000  ──→  dev-1 web
  localhost:5432  ──→  dev-1 db
```

Cambiar el checkout es instantáneo: Coast mata y vuelve a iniciar reenviadores `socat` ligeros. No se reinicia ningún contenedor.

```bash
coast checkout dev-2   # instant swap

# localhost:3000  ──→  dev-2 web
# localhost:5432  ──→  dev-2 db
```

## Nota sobre Linux

Los puertos dinámicos siempre funcionan en Linux sin privilegios especiales.

Los puertos canónicos por debajo de `1024` son diferentes. Si tu Coastfile declara puertos como `80` o `443`, Linux puede impedir que `coast checkout` los asocie hasta que configures el host. Las soluciones habituales son:

- aumentar `net.ipv4.ip_unprivileged_port_start`
- otorgar capacidad de bind al binario o proceso de reenvío

Coast informa esto explícitamente cuando el host niega la asociación.

En WSL, Coast usa bridges de checkout publicados por Docker para que los navegadores y herramientas de Windows puedan alcanzar los puertos canónicos seleccionados a través de `127.0.0.1`, de forma similar a flujos de trabajo de Docker Desktop como Sail.

Para proyectos HTTPS locales que usan Caddy, Coast reutiliza una CA local de Caddy por instalación de Coast. Después de confiar en esa raíz una vez, los workspaces recreados bajo la misma instalación siguen usándola.

El certificado raíz se encuentra en:

- `~/.coast/caddy/pki/authorities/local/root.crt` para la instalación normal
- `~/.coast-dev/caddy/pki/authorities/local/root.crt` para `coast-dev`

Estos están separados intencionalmente, así que confiar en `coast-dev` no implica también confiar en una instalación normal de `coast`, y viceversa.

Para inspeccionar o exportar el certificado raíz de la instalación activa:

```bash
coast cert info
coast cert path
coast cert fingerprint
coast cert export --to ~/Downloads/coast-root.crt
```

Coast deja la instalación de confianza en tus manos. Exporta el certificado y luego impórtalo en el almacén de confianza de tu sistema operativo o navegador según sea necesario.

## ¿Necesitas hacer checkout?

No necesariamente. Cada Coast en ejecución siempre tiene sus propios puertos dinámicos, y puedes acceder a cualquier Coast a través de esos puertos en cualquier momento sin hacer checkout de nada.

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

Puedes abrir `localhost:62217` en tu navegador para acceder al servidor web de dev-1 sin hacer checkout. Esto está perfectamente bien para muchos flujos de trabajo, y puedes ejecutar tantos Coasts como quieras sin usar nunca `coast checkout`.

## Cuándo es útil Checkout

Hay situaciones en las que los puertos dinámicos no son suficientes y necesitas puertos canónicos:

- **Aplicaciones cliente codificadas con puertos canónicos.** Si tienes un cliente ejecutándose fuera del Coast —un servidor de desarrollo frontend en tu host, una aplicación móvil en tu teléfono o una aplicación de escritorio— que espera `localhost:3000` o `localhost:8080`, cambiar los números de puerto en todas partes es poco práctico. Hacer checkout del Coast te da los puertos reales sin cambiar ninguna configuración.

- **Webhooks y URLs de callback.** Servicios como Stripe, GitHub o proveedores de OAuth envían callbacks a una URL que registraste, normalmente algo como `https://your-ngrok-tunnel.io` que reenvía a `localhost:3000`. Si cambias a un puerto dinámico, los callbacks dejan de llegar. Hacer checkout garantiza que el puerto canónico esté activo para el Coast que estás probando.

- **Herramientas de base de datos, depuradores e integraciones de IDE.** Muchos clientes GUI (pgAdmin, DataGrip, TablePlus), depuradores y configuraciones de ejecución de IDE guardan perfiles de conexión con un puerto específico. Checkout te permite mantener tus perfiles guardados y simplemente cambiar qué Coast está detrás de ellos, sin reconfigurar el destino de conexión de tu depurador o la conexión a la base de datos cada vez que cambias de contexto.

## Liberar Checkout

Si quieres liberar los puertos canónicos sin hacer checkout de un Coast diferente:

```bash
coast checkout --none
```

Después de esto, ningún Coast posee los puertos canónicos. Todos los Coasts siguen siendo accesibles a través de sus puertos dinámicos.

## Solo Uno a la Vez

Exactamente un Coast puede estar seleccionado mediante checkout a la vez. Si `dev-1` está seleccionado y ejecutas `coast checkout dev-2`, los puertos canónicos cambian instantáneamente a `dev-2`. No hay ningún intervalo: los reenviadores antiguos se terminan y los nuevos se inician en la misma operación.

```text
┌──────────────────────────────────────────────────┐
│  Your machine                                    │
│                                                  │
│  Canonical (checked-out Coast only):             │
│    localhost:3000 ──→ dev-2 web                  │
│    localhost:5432 ──→ dev-2 db                   │
│                                                  │
│  Dynamic (always available):                     │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

Los puertos dinámicos no se ven afectados por checkout. Lo único que cambia es a dónde apuntan los puertos canónicos.
