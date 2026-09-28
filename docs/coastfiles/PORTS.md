# Ports

The `[ports]` section declares which ports Coast manages for forwarding between your Coast instances and the host machine. The optional `[egress]` section declares ports on the host that Coast instances need to reach outbound.

For how port forwarding works at runtime — canonical vs dynamic ports, checkout swapping, socat — see [Ports](../concepts_and_terminology/PORTS.md) and [Checkout](../concepts_and_terminology/CHECKOUT.md).

## `[ports]`

A flat map of `logical_name = port_number`. Each entry tells Coast to set up port forwarding for that port when a Coast instance runs.

```toml
[ports]
web = 3000
api = 8080
postgres = 5432
```

Every instance gets a dynamic port (high range, always accessible) for each declared port. The [checked-out](../concepts_and_terminology/CHECKOUT.md) instance also gets the canonical port (the number you declared) forwarded to the host.

Rules:

- Port values must be non-zero unsigned 16-bit integers (1-65535).
- Logical names are freeform strings used as identifiers in `coast ports`, Coastguard, and `primary_port`.

### Minimal example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
app = 3000
```

### Multi-service example

```toml
[ports]
web = 3000
api = 4000
backend = 8080
postgres = 5432
redis = 6379
```

## `primary_port`

Set in the `[coast]` section (documented in [Project and Setup](PROJECT.md)), `primary_port` names one of your declared ports for quick-links and subdomain routing in [Coastguard](../concepts_and_terminology/COASTGUARD.md).

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

The value must match a key in `[ports]`. See [Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md) for details.

## Health indicators in Coastguard

When a port's logical name matches a Compose service name (for example, `web`),
Coastguard also uses that service's Docker health status. `running (starting)` is
amber while Docker performs its startup health checks; an unhealthy or stopped
service is red. Once the service is ready, the port must also respond to its
probe before the indicator turns green. Configure the startup grace period and
retry policy in the service's Compose `healthcheck`.

Ports without a same-name service retain probe-only indicators. If fetching
service status fails, the indicator falls back to the port probe rather than
keeping an old startup state. This display does not change when `coast run`
finishes or replace application readiness checks. These are periodically sampled
statuses, so a fast restart can remain at its previous status until the next
sample. Failed port probes and cached non-ready states trigger a fresh service
lookup in the background probe loop; healthy steady-state ports reuse the cache.

## `[egress]`

Declares ports on the host that Coast instances need to reach. This is the reverse direction from `[ports]` — instead of forwarding a port *out* of the Coast to the host, egress makes a host port reachable *from inside* the Coast.

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
app = 48090

[egress]
host-api = 48080
```

This is useful when your compose services inside a Coast need to talk to something running directly on the host machine (outside of Coast's shared services system).

Rules:

- Same as `[ports]`: values must be non-zero unsigned 16-bit integers.
- Logical names are freeform identifiers.
