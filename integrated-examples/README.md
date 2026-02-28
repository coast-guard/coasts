# Integrated Examples

Example projects and integration tests for coast's features: worktrees, assign, checkout swaps, secret injection, lookup, and more.

These tests are **not run in CI**. They require a running Docker daemon, socat, and fully built release binaries. They are designed for local development and manual verification before releases.

## Why this pattern?

Coast creates git worktrees for each instance, so every example project needs its own independent git repository. The project files live under `projects/` (gitignored), and a setup script initializes each one as a standalone git repo with feature branches for testing.

## Setup

Run the setup script from the coast repo root:

```bash
./integrated-examples/setup.sh
```

This initializes a git repo in each example directory and creates feature branches with code changes suitable for testing checkout swaps.

The script is idempotent -- re-running it resets each example to a clean state.

### Project archive

The `projects/` directory is gitignored (it contains `.git` directories). To share or restore the initialized projects across machines, use the Makefile targets:

```bash
make zip-projects      # pack projects/ into a committable zip
make unpack-projects   # unpack projects.zip, replacing projects/
```

## Running tests

Build the release binaries first:

```bash
cargo build --release
```

### Run all tests

```bash
./integrated-examples/test.sh
```

This discovers and runs every `test_*.sh` script. Each test runs in isolation with its own setup and cleanup. The `test_claude.sh` (macOS Keychain) test is skipped by default.

### Include the macOS Keychain test

```bash
./integrated-examples/test.sh --include-keychain
```

Requires a macOS Keychain entry for "Claude Code" and `coast-extractor-keychain` on PATH.

### Run specific tests

```bash
./integrated-examples/test.sh test_assign              # single test
./integrated-examples/test.sh test_checkout test_assign # multiple tests
./integrated-examples/test.sh assign checkout           # test_ prefix is optional
```

### Run a test directly

```bash
./integrated-examples/test_assign.sh
```

## Test inventory

| Test | Example project | What it covers |
|------|----------------|----------------|
| `test_lifecycle.sh` | coast-demo | Full lifecycle: build, run, stop, start, exec, rm. Shared postgres volumes, isolated redis, migration accumulation. |
| `test_assign.sh` | coast-benchmark | Seamless branch switching on a single slot via `coast assign`. 3 swaps, bidirectional, canonical port integration. |
| `test_checkout.sh` | coast-api | Checkout swap, `--none`, instant swap between instances, dynamic port independence. |
| `test_worktree.sh` | coast-demo | Worktree creation, `rm --prune`, `--no-worktree` mode. |
| `test_secrets.sh` | coast-secrets | File, env, and command extractors. Secret injection as env vars and file mounts. `coast secret list/set`. `coast build --refresh`. |
| `test_claude.sh` | coast-claude | macOS Keychain secret extraction and injection. Requires Keychain entry. Skipped by default. |
| `test_error_cases.sh` | coast-api | Duplicate names, double-stop, double-start, checkout stopped/nonexistent, rm nonexistent. |
| `test_lookup.sh` | coast-lookup | `coast lookup` worktree discovery: compact, JSON, default output, subdirectory resolution. |
| `test_multi_project.sh` | coast-demo + coast-api | Two projects simultaneously, cross-project messaging, `coast ls` with multiple roots. |
| `test_observability.sh` | coast-demo | `coast ps`, `coast logs`, `coast logs <service>`, `coast shared ls`. |

The `benchmark.sh` script is not prefixed with `test_` and is not auto-discovered. Run it manually:

```bash
COAST_BENCHMARK_COUNT=3 ./integrated-examples/benchmark.sh   # quick smoke test
./integrated-examples/benchmark.sh                            # full 50-instance benchmark
```

## Manual testing

After setup:

```bash
# Start the daemon
coastd --foreground &

# Build and run
cd integrated-examples/coast-demo
coast build
coast run main
coast run feature-greeting --worktree feature-greeting

# Test checkout swap
coast checkout main
curl http://localhost:33000/          # "Hello from Coast!"

coast checkout feature-greeting
curl http://localhost:33000/          # "Hello from Feature Branch!"
```

## Adding a new example

1. Create a new directory under `integrated-examples/` with the project files (Coastfile, docker-compose.yml, etc.).
2. Add a setup block in `setup.sh` that initializes the git repo and creates feature branches with testable differences.
3. Add the example's `.git` and `.coasts` directories to the coast repo's `.gitignore` (the wildcard patterns already handle this).

## Adding a new test

1. Create `test_<name>.sh` in this directory.
2. Follow the pattern: `source helpers.sh`, `register_cleanup`, `preflight_checks`, `clean_slate`, setup, test sections, cleanup.
3. It will be auto-discovered by `test.sh`.
