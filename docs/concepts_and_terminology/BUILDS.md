# Builds

Think of a coast build as a Docker image with extra help. A build is a directory-based artifact that bundles everything needed to create Coast instances: a resolved [Coastfile](COASTFILE_TYPES.md), a rewritten compose file, pre-pulled OCI image tarballs, and injected host files. It is not a Docker image itself, but it contains Docker images (as tarballs) plus the metadata Coast needs to wire them together.

## What `coast build` Does

When you run `coast build`, the daemon executes these steps in order:

1. Parses and validates the Coastfile.
2. Reads the compose file and filters out omitted services.
3. Extracts [secrets](SECRETS.md) from configured extractors and stores them encrypted in the keystore.
4. Builds Docker images for compose services that have `build:` directives (on the host).
5. Pulls Docker images for compose services that have `image:` directives.
6. Caches all images as OCI tarballs in `~/.coast/image-cache/`.
7. If `[coast.setup]` is configured, builds a custom DinD base image with the specified packages, commands, and files.
8. Writes the build artifact directory with the manifest, resolved coastfile, rewritten compose, and injected files.
9. Updates the `latest` symlink to point to the new build.
10. Auto-prunes old builds beyond the keep limit.

## Where Builds Live

```text
~/.coast/
  images/
    my-project/
      latest -> a3c7d783_20260227143000       (symlink)
      a3c7d783_20260227143000/                (versioned build)
        manifest.json
        coastfile.toml
        compose.yml
        inject/
      b4d8e894_20260226120000/                (older build)
        ...
  image-cache/                                (shared tarball cache)
    postgres_16_a1b2c3d4e5f6.tar
    redis_7_f6e5d4c3b2a1.tar
    coast-built_my-project_web_latest_...tar
```

Each build gets a unique **build ID** in the format `{coastfile_hash}_{YYYYMMDDHHMMSS}`. The hash incorporates the Coastfile content and resolved configuration, so changes to the Coastfile produce a new build ID.

The `latest` symlink always points to the most recent build for quick resolution. If your project uses typed Coastfiles (e.g., `Coastfile.light`), each type gets its own symlink: `latest-light`.

The image cache at `~/.coast/image-cache/` is shared across all projects. If two projects use the same Postgres image, the tarball is cached once.

## What a Build Contains

Each build directory contains:

- **`manifest.json`** -- full build metadata: project name, build timestamp, coastfile hash, list of cached/built images, secret names, omitted services, [volume strategies](VOLUMES.md), and more.
- **`coastfile.toml`** -- the resolved Coastfile (merged with parent if using `extends`).
- **`compose.yml`** -- a rewritten version of your compose file where `build:` directives are replaced with pre-built image tags, and omitted services are stripped.
- **`inject/`** -- copies of host files from `[inject].files` (e.g., `~/.gitconfig`, `~/.npmrc`).

## Builds Do Not Contain Secrets

Secrets are extracted during the build step, but they are stored in a separate encrypted keystore at `~/.coast/keystore.db` -- not inside the build artifact directory. The manifest only records the **names** of the secrets that were extracted, never the values.

This means build artifacts are safe to inspect without exposing sensitive data. Secrets are decrypted and injected later, when a Coast instance is created with `coast run`.

## Builds and Docker

A build involves three kinds of Docker images:

- **Built images** -- compose services with `build:` directives are built on the host via `docker build`, tagged as `coast-built/{project}/{service}:latest`, and saved as tarballs in the image cache.
- **Pulled images** -- compose services with `image:` directives are pulled and saved as tarballs.
- **Coast image** -- if `[coast.setup]` is configured, a custom Docker image is built on top of `docker:dind` with the specified packages, commands, and files. Tagged as `coast-image/{project}:{build_id}`.

At runtime ([`coast run`](RUN.md)), these tarballs are loaded into the inner [DinD daemon](RUNTIMES_AND_SERVICES.md) via `docker load`. This is what makes Coast instances start quickly without needing to pull images from a registry.

## Builds and Instances

When you run [`coast run`](RUN.md), Coast resolves the latest build (or a specific `--build-id`) and uses its artifacts to create the instance. The build ID is recorded on the instance.

You do not need to rebuild to create more instances. One build can serve many Coast instances running in parallel.

## When to Rebuild

Only rebuild when your Coastfile, `docker-compose.yml`, or infrastructure configuration changes. Rebuilding is resource-intensive -- it re-pulls images, re-builds Docker images, and re-extracts secrets.

Code changes do not require a rebuild. Coast mounts your project directory directly into each instance, so code updates are picked up immediately.

## Auto-Pruning

Coast keeps up to 5 builds per Coastfile type. After every successful `coast build`, older builds beyond the limit are automatically removed.

Builds that are in use by running instances are never pruned, regardless of the limit. If you have 7 builds but 3 of them are backing active instances, all 3 are protected.

## Manual Removal

You can remove builds manually via `coast rm-build` or through the Coastguard Builds tab.

- **Full project removal** (`coast rm-build <project>`) requires all instances to be stopped and removed first. It removes the entire build directory, associated Docker images, volumes, and containers.
- **Selective removal** (by build ID, available in the Coastguard UI) skips builds that are in use by running instances.

## Typed Builds

If your project uses multiple Coastfiles (e.g., `Coastfile` for the default configuration and `Coastfile.snap` for snapshot-seeded volumes), each type maintains its own `latest-{type}` symlink and its own 5-build pruning pool.

```bash
coast build              # uses Coastfile, updates "latest"
coast build --type snap  # uses Coastfile.snap, updates "latest-snap"
```

Pruning a `snap` build never touches `default` builds, and vice versa.
