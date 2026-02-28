<p align="center">
  <img src="assets/coasts_blue_bg.svg" alt="Coasts" width="160" />
</p>

# Coasts

Coasts (Containerized Hosts) is a CLI tool for running multiple isolated instances of a full development environment on a single machine. Each coast is a container running its own Docker daemon, inside which your existing `docker-compose.yml` runs completely unmodified.

Build once, run N instances, check out one at a time to seamlessly bind its ports to your host.

For the full user-facing documentation, see the [Coasts docs](docs/README.md).

To contribute, read the [contributing guide](CONTRIBUTING.md).

## Prerequisites

- Rust (stable toolchain)
- Docker
- Node.js
- socat (`brew install socat` on macOS)
- Git

## Building

```bash
cargo build --release
```

Binaries are placed in `target/release/`:
- `coast` -- the CLI client
- `coastd` -- the background daemon

## Quick Start

```bash
# Start the daemon
coastd --foreground &

# In a project with a Coastfile and docker-compose.yml:
coast build
coast run main
coast run feature-x --worktree feature/x

# Swap which instance owns the canonical ports
coast checkout main
coast checkout feature-x

# Inspect
coast ls
coast ps main
coast logs main
coast ports main

# Clean up
coast rm main
coast rm feature-x
```

## Development

### Makefile targets

The [Makefile](Makefile) is the primary entry point for development tasks:

| Command | What it does |
|---------|-------------|
| `make lint` | Check formatting (`cargo fmt --check`) and run `cargo clippy` |
| `make fix` | Auto-format and auto-fix clippy warnings |
| `make test` | Run the full unit test suite across all workspace crates |
| `make check` | `make lint` + `make test` in sequence |
| `make coverage` | Generate an HTML coverage report and open it |
| `make watch` | Rebuild on source changes (requires `cargo-watch`) |

### Coast Guard (web UI)

The web UI lives in `coast-guard/`. During development, the daemon serves it as an embedded SPA, but for hot-reloading it is easiest to run the Vite dev server directly:

```bash
cd coast-guard
npm install
npm run dev
```

This starts the UI on `http://localhost:5173` with hot module replacement. The Vite dev server proxies `/api` requests to the daemon at `localhost:31415`, so you need `coastd` running alongside.

#### Generating TypeScript types

The web UI depends on TypeScript types generated from Rust structs via `ts-rs`. After changing any Rust types that are used by the UI, regenerate the bindings:

```bash
cd coast-guard
npm run generate:types
```

This runs `cargo test -p coast-core export_bindings` and rebuilds the barrel file in `src/types/generated/`.

#### Generating the docs manifest

The docs viewer in the UI reads from a generated manifest. After changing any markdown files in `docs/`, regenerate it:

```bash
cd coast-guard
npm run generate:docs
```

### Docs localization and search indexes

Translation and search index generation are centralized Python scripts invoked via the Makefile:

```bash
make docs-status                      # show which docs need translation
make translate LOCALE=es              # translate docs for one locale
make translate-all                    # translate all supported locales
make doc-search LOCALE=en             # generate search index for one locale
make doc-search-all                   # generate search indexes for all locales
```

Both scripts read `OPENAI_API_KEY` from the environment or from `.env` in the project root. See `.env.example`.

## Testing

### Unit tests

```bash
make test
```

Runs `cargo test --workspace` across all crates.

### Integration tests

Integration tests live in `integrated-examples/` and exercise full end-to-end coast workflows. They are useful for validating real behavior but come with practical costs: they require Docker running, socat installed, and a release build. Each test spins up real DinD containers, so a full run can consume significant disk space and you may need to `docker system prune` periodically to reclaim it.

For the full list of tests, prerequisites, and cleanup guidance, see the [integrated-examples README](integrated-examples/README.md).

Quick usage:

```bash
integrated-examples/test.sh                            # run all tests
integrated-examples/test.sh test_checkout test_secrets  # run specific tests
integrated-examples/test.sh --include-keychain          # include macOS Keychain test
```

## Project Structure

```
coast/
  coast-cli/          # Thin CLI client, talks to daemon over unix socket
  coast-daemon/       # coastd background process (handlers, state DB, port manager)
  coast-core/         # Shared types, Coastfile parsing, protocol definitions
  coast-secrets/      # Secret extraction, encryption, keystore
  coast-docker/       # Docker API wrapper, DinD runtime, compose interaction
  coast-git/          # Git worktree management
  coast-guard/        # Web UI (React + Vite), served by the daemon
  coast-i18n/         # i18n locale files for the CLI
  scripts/            # Python build scripts (translation, search index generation)
  docs/               # User-facing documentation (English + translations)
  integrated-examples/  # Example projects and shell-based integration tests
```

