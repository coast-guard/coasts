# Puertos

La sección `[ports]` declara qué puertos gestiona Coast para el reenvío entre tus instancias de Coast y la máquina host. La sección opcional `[egress]` declara puertos en el host a los que las instancias de Coast necesitan acceder hacia afuera.

Para entender cómo funciona el reenvío de puertos en tiempo de ejecución — puertos canónicos vs dinámicos, intercambio de checkout, socat — consulta [Ports](../concepts_and_terminology/PORTS.md) y [Checkout](../concepts_and_terminology/CHECKOUT.md).

## `[ports]`

Un mapa plano de `logical_name = port_number`. Cada entrada le indica a Coast que configure el reenvío de puertos para ese puerto cuando se ejecute una instancia de Coast.

```toml
[ports]
web = 3000
api = 8080
postgres = 5432
```

Cada instancia obtiene un puerto dinámico (rango alto, siempre accesible) para cada puerto declarado. La instancia [checked-out](../concepts_and_terminology/CHECKOUT.md) también obtiene el puerto canónico (el número que declaraste) reenviado al host.

Reglas:

- Los valores de puerto deben ser enteros sin signo de 16 bits distintos de cero (1-65535).
- Los nombres lógicos son cadenas de formato libre usadas como identificadores en `coast ports`, Coastguard y `primary_port`.

### Ejemplo mínimo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
app = 3000
```

### Ejemplo multi-servicio

```toml
[ports]
web = 3000
api = 4000
backend = 8080
postgres = 5432
redis = 6379
```

## `primary_port`

Configurado en la sección `[coast]` (documentada en [Project and Setup](PROJECT.md)), `primary_port` nombra uno de tus puertos declarados para enlaces rápidos y enrutamiento por subdominio en [Coastguard](../concepts_and_terminology/COASTGUARD.md).

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

El valor debe coincidir con una clave en `[ports]`. Consulta [Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md) para más detalles.

## `[egress]`

Declara puertos en el host a los que las instancias de Coast necesitan acceder. Esta es la dirección inversa de `[ports]` — en lugar de reenviar un puerto *fuera* de Coast hacia el host, egress hace que un puerto del host sea accesible *desde dentro* de Coast.

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
app = 48090

[egress]
host-api = 48080
```

Esto es útil cuando tus servicios de compose dentro de una Coast necesitan hablar con algo que se está ejecutando directamente en la máquina host (fuera del sistema de servicios compartidos de Coast).

Reglas:

- Igual que `[ports]`: los valores deben ser enteros sin signo de 16 bits distintos de cero.
- Los nombres lógicos son identificadores de formato libre.
