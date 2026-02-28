# Checkout

Checkout controls which Coast instance owns your project's [canonical ports](PORTS.md). When you check out a Coast, `localhost:3000`, `localhost:5432`, and every other canonical port maps straight to that instance.

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

Switching checkout is instant — Coast kills and respawns lightweight `socat` forwarders. No containers are restarted.

```bash
coast checkout dev-2   # instant swap

# localhost:3000  ──→  dev-2 web
# localhost:5432  ──→  dev-2 db
```

## Do You Need to Check Out?

Not necessarily. Every running Coast always has its own dynamic ports, and you can access any Coast through those ports at any time without checking anything out.

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

You can open `localhost:62217` in your browser to hit dev-1's web server without checking it out. This is perfectly fine for many workflows, and you can run as many Coasts as you want without ever using `coast checkout`.

## When Checkout Is Useful

There are situations where dynamic ports are not enough and you need canonical ports:

- **Client applications hardcoded to canonical ports.** If you have a client running outside the Coast — a frontend dev server on your host, a mobile app on your phone, or a desktop app — that expects `localhost:3000` or `localhost:8080`, changing port numbers everywhere is impractical. Checking out the Coast gives you the real ports without changing any configuration.

- **Webhooks and callback URLs.** Services like Stripe, GitHub, or OAuth providers send callbacks to a URL you registered — usually something like `https://your-ngrok-tunnel.io` that forwards to `localhost:3000`. If you switch to a dynamic port, the callbacks stop arriving. Checking out ensures the canonical port is active for the Coast you are testing.

- **Database tools, debuggers, and IDE integrations.** Many GUI clients (pgAdmin, DataGrip, TablePlus), debuggers, and IDE run configurations save connection profiles with a specific port. Checkout lets you keep your saved profiles and just swap which Coast is behind them — no reconfiguring your debugger attach target or database connection every time you switch contexts.

## Releasing Checkout

If you want to release the canonical ports without checking out a different Coast:

```bash
coast checkout --none
```

After this, no Coast owns the canonical ports. All Coasts remain accessible through their dynamic ports.

## Only One at a Time

Exactly one Coast can be checked out at a time. If `dev-1` is checked out and you run `coast checkout dev-2`, the canonical ports instantly swap to `dev-2`. There is no gap — the old forwarders are killed and new ones are spawned in the same operation.

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

Dynamic ports are unaffected by checkout. The only thing that changes is where the canonical ports point.
