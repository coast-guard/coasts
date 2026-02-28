# Shared Services

Shared services are database and infrastructure containers (Postgres, Redis, MongoDB, etc.) that run on your host Docker daemon rather than inside a Coast. Coast instances connect to them over a bridge network, so every Coast talks to the same service on the same host volume.

![Shared services in Coastguard](../../assets/coastguard-shared-services.png)
*The Coastguard shared services tab showing host-managed Postgres, Redis, and MongoDB.*

## How They Work

When you declare a shared service in your Coastfile, Coast starts it on the host daemon and removes it from the compose stack that runs inside each Coast container. Coasts are then configured to route their connections back to the host.

```text
Host Docker daemon
  |
  +--> postgres (host volume: infra_postgres_data)
  +--> redis    (host volume: infra_redis_data)
  +--> mongodb  (host volume: infra_mongodb_data)
  |
  +--> Coast: dev-1  --bridge network--> host postgres, redis, mongodb
  +--> Coast: dev-2  --bridge network--> host postgres, redis, mongodb
```

Because shared services reuse your existing host volumes, any data you already have from running `docker-compose up` locally is immediately available to your Coasts.

## When to Use Shared Services

- Your project has MCP integrations that connect to a local database — shared services let those continue to work without reconfiguration. A database MCP on your host that connects to `localhost:5432` keeps working because the shared Postgres is on the host at that same port. No dynamic port discovery, no MCP reconfiguration. See [MCP Servers](MCP_SERVERS.md) for more on this.
- You want lighter Coast instances since they do not need to run their own database containers.
- You do not need data isolation between Coast instances (every instance sees the same data).
- You are running coding agents on the host (see [Filesystem](FILESYSTEM.md)) and want them to access database state without routing through [`coast exec`](EXEC_AND_DOCKER.md). With shared services, the agent's existing database tools and MCPs work unchanged.

See the [Volume Topology](VOLUMES.md) page for alternatives when you do need isolation.

## Volume Disambiguation Warning

Docker volume names are not always globally unique. If you run `docker-compose up` from multiple different projects, the host volumes that Coast attaches to shared services may not be the ones you expect.

Before starting Coasts with shared services, make sure the last `docker-compose up` you ran was from the project you intend to use with Coasts. This ensures the host volumes match what your Coastfile expects.

## Troubleshooting

If your shared services appear to be pointing at the wrong host volume:

1. Open the [Coastguard](COASTGUARD.md) UI (`coast ui`).
2. Navigate to the **Shared Services** tab.
3. Select the affected services and click **Remove**.
4. Click **Refresh Shared Services** to recreate them from your current Coastfile configuration.

This tears down and recreates the shared service containers, reattaching them to the correct host volumes.
