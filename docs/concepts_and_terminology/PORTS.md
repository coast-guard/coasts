# Ports

Coast manages two kinds of port mappings for every service in a Coast instance: canonical ports and dynamic ports.

## Canonical Ports

These are the ports your project normally runs on — the ones in your `docker-compose.yml` or local dev config. For example, `3000` for a web server, `5432` for Postgres.

Only one Coast can have canonical ports at a time. Whichever Coast is [checked out](CHECKOUT.md) gets them.

```text
coast checkout dev-1

localhost:3000  ──→  dev-1
localhost:5432  ──→  dev-1
```

This means your browser, API clients, database tools, and test suites all work exactly as they normally would — no port number changes needed.

On Linux, canonical ports below `1024` may require host configuration before [`coast checkout`](CHECKOUT.md) can bind them. Dynamic ports do not have this restriction.

## Dynamic Ports

Every running Coast always gets its own set of dynamic ports in a high range (49152–65535). These are assigned automatically and are always accessible, regardless of which Coast is checked out.

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

Dynamic ports let you peek at any Coast without checking it out. You can open `localhost:63104` to hit dev-2's web server while dev-1 is checked out on the canonical ports.

## How They Work Together

```text
┌──────────────────────────────────────────────────┐
│  Your machine                                    │
│                                                  │
│  Canonical (checked-out Coast only):             │
│    localhost:3000 ──→ dev-1 web                  │
│    localhost:5432 ──→ dev-1 db                   │
│                                                  │
│  Dynamic (always available):                     │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

Switching [checkout](CHECKOUT.md) is instant. Coast kills and respawns lightweight `socat` forwarders. No containers are restarted.

## Dynamic Port Environment Variables

Coast injects environment variables into every instance that expose the dynamic port for each service. The variable name is derived from the `[ports]` key: `web` becomes `WEB_DYNAMIC_PORT`, `backend-test` becomes `BACKEND_TEST_DYNAMIC_PORT`.

These are useful when a service needs to know its externally-reachable port, for example to set `AUTH_URL` for auth callback redirects. See [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) for the full reference.

## See Also

- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - quick-links, subdomain routing, and URL templates
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - using `WEB_DYNAMIC_PORT` and related variables in service commands
