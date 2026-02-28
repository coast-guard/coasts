# Primary Port & DNS

The primary port is an optional convenience feature that creates a quick-link to one of your services — typically your web frontend. It shows up as a clickable badge in Coastguard and a starred entry in `coast ports`. It does not change how ports work; it just picks one to highlight.

## Setting the Primary Port

Add `primary_port` to the `[coast]` section of your Coastfile, referencing a key from [`[ports]`](PORTS.md):

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
backend = 8080
```

If your project has only one port, Coast auto-detects it as the primary — you do not need to set it explicitly.

You can also toggle the primary from the Coastguard Ports tab by clicking the star icon next to any service, or from the CLI with `coast ports set-primary`. The setting is per-build, so all instances created from the same build share the same primary.

## What It Enables

```text
coast ports dev-1

  SERVICE    CANONICAL  DYNAMIC
  ★ web      3000       62217
    backend  8080       63889
```

The starred service is your primary. In Coastguard, it appears as a clickable badge next to the instance name — one click opens your app in the browser.

This is particularly useful for:

- **Host-side agents** — give your AI agent a single URL to check changes against. Instead of telling it "open localhost:62217", the primary port URL is available programmatically from `coast ls` and the daemon API.
- **Browser MCPs** — if your agent uses a browser MCP to verify UI changes, the primary port URL is the canonical target to point it at.
- **Quick iteration** — one-click access to the service you look at most often.

The primary port is entirely optional. Everything works without it — it is a quality-of-life feature for faster navigation.

## Subdomain Routing

When you run multiple Coast instances with isolated databases, they all share `localhost` in the browser. This means cookies set by `localhost:62217` (dev-1) are visible to `localhost:63104` (dev-2). If your app uses session cookies, logging into one instance can interfere with another.

Subdomain routing solves this by giving each instance its own origin:

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

Enable it per-project from the Coastguard Ports tab (toggle at the bottom of the page) or via the daemon settings API.

### Tradeoff: CORS

The downside is that your application may need CORS adjustments. If your frontend at `dev-1.localhost:3000` makes API requests to `dev-1.localhost:8080`, the browser treats these as cross-origin because the port differs. Most dev servers already handle this, but if you see CORS errors after enabling subdomain routing, check your application's allowed origins configuration.

## URL Templates

Each service has a URL template that controls how its links are generated. The default is:

```text
http://localhost:<port>
```

The `<port>` placeholder is replaced with the actual port number — the canonical port when the instance is [checked out](CHECKOUT.md), or the dynamic port otherwise. When subdomain routing is enabled, `localhost:` is replaced with `{instance}.localhost:`.

You can customize templates per-service from the Coastguard Ports tab (pencil icon next to each service). This is useful if your dev server uses HTTPS, a custom hostname, or a non-standard URL scheme:

```text
https://my-service.localhost:<port>
```

Templates are stored in the daemon settings and persist across restarts.

## DNS Setup

Most browsers resolve `*.localhost` to `127.0.0.1` out of the box, so subdomain routing works without any DNS configuration.

If you need custom domain resolution (e.g. `*.localcoast`), Coast includes an embedded DNS server. Set it up once:

```bash
coast dns setup    # writes /etc/resolver/localcoast (requires sudo)
coast dns status   # check if DNS is configured
coast dns remove   # remove the resolver entry
```

This is optional and only needed if `*.localhost` does not work in your browser or you want a custom TLD.
