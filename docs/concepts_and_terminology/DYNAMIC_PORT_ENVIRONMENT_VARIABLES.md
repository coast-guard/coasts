# Dynamic Port Environment Variables

Every Coast instance gets a set of environment variables that expose the [dynamic port](PORTS.md) assigned to each service. These variables are available inside both bare services and compose containers, and they let your application discover its externally-reachable port at runtime.

## Naming Convention

Coast derives the variable name from the logical service name in your `[ports]` section:

1. Convert to uppercase
2. Replace non-alphanumeric characters with underscores
3. Append `_DYNAMIC_PORT`

```text
[ports] key          Environment variable
─────────────        ────────────────────────────
web             →    WEB_DYNAMIC_PORT
postgres        →    POSTGRES_DYNAMIC_PORT
backend-test    →    BACKEND_TEST_DYNAMIC_PORT
svc.v2          →    SVC_V2_DYNAMIC_PORT
```

If the service name starts with a digit, Coast prefixes the variable with an underscore (e.g. `9svc` becomes `_9SVC_DYNAMIC_PORT`). An empty name falls back to `SERVICE_DYNAMIC_PORT`.

## Example

Given this Coastfile:

```toml
[ports]
web = 3000
api = 8080
postgres = 5432
```

Every Coast instance created from this build will have three additional environment variables:

```text
WEB_DYNAMIC_PORT=62217
API_DYNAMIC_PORT=55681
POSTGRES_DYNAMIC_PORT=56905
```

The actual port numbers are assigned at `coast run` time and differ per instance.

## When to Use Them

The most common use case is configuring services that embed their own URL in responses: auth callbacks, OAuth redirect URIs, CORS origins, or webhook URLs. These services need to know the port that external clients use, not the internal port they listen on.

For example, a Next.js application using NextAuth needs `AUTH_URL` set to the externally-reachable address. Inside the Coast, Next.js always listens on port 3000, but the host-side port is dynamic:

```toml
[services.web]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} yarn dev:web"
port = 3000
```

The `:-3000` fallback means the command also works outside of Coast, where `WEB_DYNAMIC_PORT` is not set.

## Precedence

If an environment variable with the same name already exists in the Coast container (set via secrets, inject, or compose environment), Coast does not overwrite it. The existing value takes precedence.

## Availability

Dynamic port variables are injected into the Coast container's environment at startup. They are available to:

- Bare service `install` commands
- Bare service `command` processes
- Compose service containers (via the container environment)
- Commands run through `coast exec`

The values do not change for the lifetime of the instance. If you stop and start the instance, it keeps the same dynamic ports.

## See Also

- [Ports](PORTS.md) - canonical vs dynamic ports and how checkout swaps between them
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - subdomain routing and cookie isolation across instances
