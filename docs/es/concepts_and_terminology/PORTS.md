# Puertos

Coast gestiona dos tipos de mapeos de puertos para cada servicio en una instancia de Coast: puertos canónicos y puertos dinámicos.

## Puertos Canónicos

Estos son los puertos en los que tu proyecto normalmente se ejecuta — los que están en tu `docker-compose.yml` o en la configuración local de desarrollo. Por ejemplo, `3000` para un servidor web, `5432` para Postgres.

Solo un Coast puede tener puertos canónicos a la vez. El Coast que esté [checked out](CHECKOUT.md) los obtiene.

```text
coast checkout dev-1

localhost:3000  ──→  dev-1
localhost:5432  ──→  dev-1
```

Esto significa que tu navegador, clientes de API, herramientas de base de datos y suites de pruebas funcionan exactamente como lo harían normalmente — sin necesidad de cambiar números de puerto.

## Puertos Dinámicos

Cada Coast en ejecución siempre obtiene su propio conjunto de puertos dinámicos en un rango alto (49152–65535). Estos se asignan automáticamente y siempre son accesibles, independientemente de qué Coast esté checked out.

```text
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681

coast ports dev-2

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       63104
#   db       5432       57220
```

Los puertos dinámicos te permiten echar un vistazo a cualquier Coast sin hacer checkout. Puedes abrir `localhost:63104` para acceder al servidor web de dev-2 mientras dev-1 está checked out en los puertos canónicos.

## Cómo Funcionan Juntos

```text
┌──────────────────────────────────────────────────┐
│  Tu máquina                                      │
│                                                  │
│  Canónicos (solo el Coast con checkout):         │
│    localhost:3000 ──→ dev-1 web                  │
│    localhost:5432 ──→ dev-1 db                   │
│                                                  │
│  Dinámicos (siempre disponibles):                │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

Cambiar el [checkout](CHECKOUT.md) es instantáneo — Coast mata y vuelve a lanzar reenviadores ligeros de `socat`. No se reinicia ningún contenedor.

Consulta también [Puerto Principal y DNS](PRIMARY_PORT_AND_DNS.md) para enlaces rápidos, enrutamiento por subdominio y plantillas de URL.
