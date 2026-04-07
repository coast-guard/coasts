# Remote Builds

Remote builds run on the remote machine via coast-service. This ensures the build uses the remote's native architecture (e.g., x86_64 on an EC2 instance) regardless of your local architecture (e.g., ARM Mac). No cross-compilation or architecture emulation is needed.

## How It Works

When you run `coast build --type remote`, the following happens:

1. The daemon rsyncs project source files (Coastfile, compose.yml, Dockerfiles, inject/) to the remote workspace via SSH.
2. The daemon calls `POST /build` on coast-service over the SSH tunnel.
3. coast-service runs the full build natively on the remote: `docker build`, image pulling, image caching, and secret extraction, all under `/data/images/`.
4. coast-service returns a `BuildResponse` with the artifact path and build metadata.
5. The daemon rsyncs the complete artifact directory (coastfile.toml, compose.yml, manifest.json, secrets/, inject/, image tarballs) back to `~/.coast/images/{project}/{build_id}/` on your local machine.
6. The daemon creates a `latest-remote` symlink pointing to the new build.

```text
Local Machine                              Remote Machine
┌─────────────────────────────┐            ┌───────────────────────────┐
│  ~/.coast/images/my-app/    │            │  /data/images/my-app/     │
│    latest-remote -> {id}    │  ◀─rsync─  │    {id}/                  │
│    {id}/                    │            │      manifest.json        │
│      manifest.json          │            │      coastfile.toml       │
│      coastfile.toml         │            │      compose.yml          │
│      compose.yml            │            │      *.tar (images)       │
│      *.tar (images)         │            │                           │
└─────────────────────────────┘            └───────────────────────────┘
```

## Commands

```bash
# Build on the default remote (auto-selected if only one registered)
coast build --type remote

# Build on a specific remote
coast build --type remote --remote my-vm

# Build without running (standalone)
coast build --type remote
```

`coast run --type remote` also triggers a build if no compatible build exists yet.

## Architecture Matching

Each build's `manifest.json` records the architecture it was built for (e.g., `aarch64`, `x86_64`). When you `coast run --type remote`, the daemon checks if an existing build matches the target remote's architecture:

- **Architecture matches**: the build is reused. No rebuild needed.
- **Architecture does not match**: the daemon searches for the newest build with the correct architecture. If none exists, it returns an error with guidance to rebuild.

This means you can build once on an x86_64 remote, and deploy to any number of x86_64 remotes without rebuilding. But you cannot use an ARM build on an x86_64 remote or vice versa.

## Symlinks

Remote builds use a separate symlink from local builds:

| Symlink | Points to |
|---------|-----------|
| `latest` | Most recent local build |
| `latest-remote` | Most recent remote build |
| `latest-{type}` | Most recent local build of a specific Coastfile type |

The separation prevents a remote build from overriding your local `latest` symlink or vice versa.

## Auto-Pruning

Coast keeps up to 5 remote builds per `(coastfile_type, architecture)` pair. After every successful remote build, older builds beyond the limit are automatically removed.

Builds that are in use by running instances are never pruned, regardless of the limit. If you have 7 x86_64 remote builds but 3 of them are backing active instances, all 3 are protected.

Pruning is architecture-aware: if you have both `aarch64` and `x86_64` remote builds, each architecture maintains its own 5-build pool independently.

## Artifact Storage

Remote build artifacts are stored in two places:

| Location | Path | Purpose |
|----------|------|---------|
| Remote | `/data/images/{project}/{build_id}/` | Source of truth on the remote machine |
| Local | `~/.coast/images/{project}/{build_id}/` | Local cache for reuse across remotes |

The image cache at `/data/image-cache/` on the remote is shared across all projects, just like `~/.coast/image-cache/` locally.

## Relationship to Local Builds

Remote builds and local builds are independent. A `coast build` (without `--type remote`) always builds on your local machine and updates the `latest` symlink. A `coast build --type remote` always builds on the remote machine and updates the `latest-remote` symlink.

You can have both local and remote builds of the same project coexisting. Local coasts use local builds; remote coasts use remote builds.

For more on how builds work in general (manifest structure, image caching, typed builds), see [Builds](../concepts_and_terminology/BUILDS.md).
