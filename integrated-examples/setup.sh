#!/usr/bin/env bash
#
# Initialize integrated examples as independent git repos with feature branches.
#
# Each example needs its own git repo for coast's branch management to work.
# This script creates the repos and feature branches with testable code changes.
#
# Usage:
#   ./integrated_examples/setup.sh          # from coast repo root
#   ./setup.sh                              # from integrated_examples/
#
# Idempotent: re-running resets each example to a clean state.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECTS_DIR="$SCRIPT_DIR/projects"
mkdir -p "$PROJECTS_DIR"

# --- coast-demo ---
# A Node.js app with Postgres (shared volume) and Redis (isolated volume).
# Two feature branches add different database migrations and endpoints.
# Tests verify shared postgres (tables accumulate) and isolated redis (fresh per instance).

setup_coast_demo() {
    local dir="$PROJECTS_DIR/coast-demo"
    echo "Setting up coast-demo..."

    # Clean any existing git state for idempotency
    rm -rf "$dir/.git" "$dir/docker-compose.override.yml"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: products table, shared pg, isolated redis"

    # --- Feature branch: feature-users ---
    # Adds a users table and /users endpoints
    git checkout -b feature-users
    cat > server.js << 'FEATURE_USERS_EOF'
const http = require("http");
const { Pool } = require("pg");
const { createClient } = require("redis");

const PORT = 3000;

const pgPool = new Pool({ connectionString: process.env.DATABASE_URL });
const redisClient = createClient({ url: process.env.REDIS_URL });

async function migrate() {
  await pgPool.query(`
    CREATE TABLE IF NOT EXISTS products (
      id SERIAL PRIMARY KEY,
      name TEXT NOT NULL,
      created_at TIMESTAMPTZ DEFAULT NOW()
    )
  `);
  await pgPool.query(`
    CREATE TABLE IF NOT EXISTS users (
      id SERIAL PRIMARY KEY,
      email TEXT NOT NULL UNIQUE,
      name TEXT,
      created_at TIMESTAMPTZ DEFAULT NOW()
    )
  `);
}

async function init() {
  await redisClient.connect();
  await migrate();
  console.log("Migrations complete. Connected to Postgres and Redis.");
}

const server = http.createServer(async (req, res) => {
  const json = (data, status = 200) => {
    res.writeHead(status, { "Content-Type": "application/json" });
    res.end(JSON.stringify(data));
  };

  try {
    if (req.url === "/health") {
      return json({ status: "ok" });
    }

    if (req.url === "/users" && req.method === "POST") {
      let body = "";
      for await (const chunk of req) body += chunk;
      const { email, name } = JSON.parse(body);
      const result = await pgPool.query(
        "INSERT INTO users (email, name) VALUES ($1, $2) RETURNING *",
        [email, name]
      );
      return json({ user: result.rows[0] }, 201);
    }

    if (req.url === "/users") {
      const result = await pgPool.query("SELECT * FROM users ORDER BY id");
      return json({ users: result.rows });
    }

    if (req.url === "/products" && req.method === "POST") {
      let body = "";
      for await (const chunk of req) body += chunk;
      const { name } = JSON.parse(body);
      const result = await pgPool.query(
        "INSERT INTO products (name) VALUES ($1) RETURNING *",
        [name]
      );
      return json({ product: result.rows[0] }, 201);
    }

    if (req.url === "/products") {
      const result = await pgPool.query("SELECT * FROM products ORDER BY id");
      return json({ products: result.rows });
    }

    if (req.url === "/tables") {
      const result = await pgPool.query(`
        SELECT table_name FROM information_schema.tables
        WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
        ORDER BY table_name
      `);
      return json({ tables: result.rows.map((r) => r.table_name) });
    }

    if (req.url === "/redis-info") {
      const marker = await redisClient.get("instance_marker");
      const counter = await redisClient.get("hit_counter");
      return json({
        instance_marker: marker,
        hit_counter: counter ? parseInt(counter) : 0,
      });
    }

    if (req.url === "/redis-set-marker") {
      await redisClient.set("instance_marker", "feature-users");
      const count = await redisClient.incr("hit_counter");
      return json({ marker: "feature-users", hit_counter: count });
    }

    // Default: homepage
    const count = await redisClient.incr("hit_counter");
    const pgResult = await pgPool.query("SELECT COUNT(*) FROM products");
    const userResult = await pgPool.query("SELECT COUNT(*) FROM users");
    return json({
      message: "Hello from Feature Users!",
      branch: "feature-users",
      redis_hits: count,
      product_count: parseInt(pgResult.rows[0].count),
      user_count: parseInt(userResult.rows[0].count),
    });
  } catch (err) {
    console.error("Request error:", err);
    json({ error: err.message }, 500);
  }
});

init()
  .then(() => {
    server.listen(PORT, () => {
      console.log(`Coast demo listening on :${PORT}`);
    });
  })
  .catch((err) => {
    console.error("Failed to start:", err);
    process.exit(1);
  });
FEATURE_USERS_EOF
    cat > test.js << 'FEATURE_USERS_TEST_EOF'
// Coast-demo branch tests (feature-users branch).
//
// Tests products table, users table, and redis isolation.
// The users table is the migration unique to this feature branch.

const { Pool } = require("pg");
const { createClient } = require("redis");

const pgPool = new Pool({ connectionString: process.env.DATABASE_URL });
const redisClient = createClient({ url: process.env.REDIS_URL });

let passed = 0;
let failed = 0;

function assert(condition, msg) {
  if (condition) {
    console.log(`  PASS: ${msg}`);
    passed++;
  } else {
    console.log(`  FAIL: ${msg}`);
    failed++;
  }
}

async function run() {
  await redisClient.connect();

  console.log("=== Feature-users branch tests ===");
  console.log("");

  // --- Postgres: table existence ---
  const tables = await pgPool.query(`
    SELECT table_name FROM information_schema.tables
    WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
    ORDER BY table_name
  `);
  const tableNames = tables.rows.map((r) => r.table_name);
  assert(tableNames.includes("products"), "products table exists");
  assert(tableNames.includes("users"), "users table exists (feature-users migration)");

  // --- Postgres: CRUD on products ---
  await pgPool.query("DELETE FROM products WHERE name = '__test_widget__'");
  const pIns = await pgPool.query(
    "INSERT INTO products (name) VALUES ('__test_widget__') RETURNING *"
  );
  assert(pIns.rows.length === 1, "inserted test product");
  const pSel = await pgPool.query(
    "SELECT * FROM products WHERE name = '__test_widget__'"
  );
  assert(pSel.rows.length === 1, "queried test product");
  await pgPool.query("DELETE FROM products WHERE name = '__test_widget__'");

  // --- Postgres: CRUD on users (feature-users specific) ---
  await pgPool.query("DELETE FROM users WHERE email = '__test__@coast.dev'");
  const uIns = await pgPool.query(
    "INSERT INTO users (email, name) VALUES ('__test__@coast.dev', 'Test User') RETURNING *"
  );
  assert(uIns.rows.length === 1, "inserted test user");
  assert(uIns.rows[0].email === "__test__@coast.dev", "user email correct");
  assert(uIns.rows[0].name === "Test User", "user name correct");

  const uSel = await pgPool.query(
    "SELECT * FROM users WHERE email = '__test__@coast.dev'"
  );
  assert(uSel.rows.length === 1, "queried test user");

  // Test unique constraint on email
  let dupError = false;
  try {
    await pgPool.query(
      "INSERT INTO users (email, name) VALUES ('__test__@coast.dev', 'Dup')"
    );
  } catch (err) {
    dupError = true;
  }
  assert(dupError, "users email unique constraint enforced");

  await pgPool.query("DELETE FROM users WHERE email = '__test__@coast.dev'");

  // --- Redis: write/read ---
  await redisClient.set("__test_key__", "hello_from_feature_users");
  const val = await redisClient.get("__test_key__");
  assert(val === "hello_from_feature_users", "redis write/read works");
  await redisClient.del("__test_key__");

  // --- Redis: isolation check ---
  const marker = await redisClient.get("instance_marker");
  assert(
    marker === null || marker === "feature-users",
    "redis has no foreign instance marker (isolated)"
  );

  // --- Cleanup ---
  await redisClient.quit();
  await pgPool.end();

  console.log("");
  console.log(`${passed} passed, ${failed} failed`);
  process.exit(failed > 0 ? 1 : 0);
}

run().catch((err) => {
  console.error("Test error:", err);
  process.exit(1);
});
FEATURE_USERS_TEST_EOF
    git add server.js test.js
    git commit -m "feature: add users table, endpoints, and tests"

    # --- Feature branch: feature-orders ---
    # Diverges from main (not from feature-users)
    git checkout main
    git checkout -b feature-orders
    cat > server.js << 'FEATURE_ORDERS_EOF'
const http = require("http");
const { Pool } = require("pg");
const { createClient } = require("redis");

const PORT = 3000;

const pgPool = new Pool({ connectionString: process.env.DATABASE_URL });
const redisClient = createClient({ url: process.env.REDIS_URL });

async function migrate() {
  await pgPool.query(`
    CREATE TABLE IF NOT EXISTS products (
      id SERIAL PRIMARY KEY,
      name TEXT NOT NULL,
      created_at TIMESTAMPTZ DEFAULT NOW()
    )
  `);
  await pgPool.query(`
    CREATE TABLE IF NOT EXISTS orders (
      id SERIAL PRIMARY KEY,
      product_name TEXT NOT NULL,
      quantity INTEGER NOT NULL DEFAULT 1,
      created_at TIMESTAMPTZ DEFAULT NOW()
    )
  `);
}

async function init() {
  await redisClient.connect();
  await migrate();
  console.log("Migrations complete. Connected to Postgres and Redis.");
}

const server = http.createServer(async (req, res) => {
  const json = (data, status = 200) => {
    res.writeHead(status, { "Content-Type": "application/json" });
    res.end(JSON.stringify(data));
  };

  try {
    if (req.url === "/health") {
      return json({ status: "ok" });
    }

    if (req.url === "/orders" && req.method === "POST") {
      let body = "";
      for await (const chunk of req) body += chunk;
      const { product_name, quantity } = JSON.parse(body);
      const result = await pgPool.query(
        "INSERT INTO orders (product_name, quantity) VALUES ($1, $2) RETURNING *",
        [product_name, quantity || 1]
      );
      return json({ order: result.rows[0] }, 201);
    }

    if (req.url === "/orders") {
      const result = await pgPool.query("SELECT * FROM orders ORDER BY id");
      return json({ orders: result.rows });
    }

    if (req.url === "/products" && req.method === "POST") {
      let body = "";
      for await (const chunk of req) body += chunk;
      const { name } = JSON.parse(body);
      const result = await pgPool.query(
        "INSERT INTO products (name) VALUES ($1) RETURNING *",
        [name]
      );
      return json({ product: result.rows[0] }, 201);
    }

    if (req.url === "/products") {
      const result = await pgPool.query("SELECT * FROM products ORDER BY id");
      return json({ products: result.rows });
    }

    if (req.url === "/tables") {
      const result = await pgPool.query(`
        SELECT table_name FROM information_schema.tables
        WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
        ORDER BY table_name
      `);
      return json({ tables: result.rows.map((r) => r.table_name) });
    }

    if (req.url === "/redis-info") {
      const marker = await redisClient.get("instance_marker");
      const counter = await redisClient.get("hit_counter");
      return json({
        instance_marker: marker,
        hit_counter: counter ? parseInt(counter) : 0,
      });
    }

    if (req.url === "/redis-set-marker") {
      await redisClient.set("instance_marker", "feature-orders");
      const count = await redisClient.incr("hit_counter");
      return json({ marker: "feature-orders", hit_counter: count });
    }

    // Default: homepage
    const count = await redisClient.incr("hit_counter");
    const pgResult = await pgPool.query("SELECT COUNT(*) FROM products");
    const orderResult = await pgPool.query("SELECT COUNT(*) FROM orders");
    return json({
      message: "Hello from Feature Orders!",
      branch: "feature-orders",
      redis_hits: count,
      product_count: parseInt(pgResult.rows[0].count),
      order_count: parseInt(orderResult.rows[0].count),
    });
  } catch (err) {
    console.error("Request error:", err);
    json({ error: err.message }, 500);
  }
});

init()
  .then(() => {
    server.listen(PORT, () => {
      console.log(`Coast demo listening on :${PORT}`);
    });
  })
  .catch((err) => {
    console.error("Failed to start:", err);
    process.exit(1);
  });
FEATURE_ORDERS_EOF
    cat > test.js << 'FEATURE_ORDERS_TEST_EOF'
// Coast-demo branch tests (feature-orders branch).
//
// Tests products table, orders table, and redis isolation.
// The orders table is the migration unique to this feature branch.

const { Pool } = require("pg");
const { createClient } = require("redis");

const pgPool = new Pool({ connectionString: process.env.DATABASE_URL });
const redisClient = createClient({ url: process.env.REDIS_URL });

let passed = 0;
let failed = 0;

function assert(condition, msg) {
  if (condition) {
    console.log(`  PASS: ${msg}`);
    passed++;
  } else {
    console.log(`  FAIL: ${msg}`);
    failed++;
  }
}

async function run() {
  await redisClient.connect();

  console.log("=== Feature-orders branch tests ===");
  console.log("");

  // --- Postgres: table existence ---
  const tables = await pgPool.query(`
    SELECT table_name FROM information_schema.tables
    WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
    ORDER BY table_name
  `);
  const tableNames = tables.rows.map((r) => r.table_name);
  assert(tableNames.includes("products"), "products table exists");
  assert(tableNames.includes("orders"), "orders table exists (feature-orders migration)");

  // --- Postgres: CRUD on products ---
  await pgPool.query("DELETE FROM products WHERE name = '__test_widget__'");
  const pIns = await pgPool.query(
    "INSERT INTO products (name) VALUES ('__test_widget__') RETURNING *"
  );
  assert(pIns.rows.length === 1, "inserted test product");
  const pSel = await pgPool.query(
    "SELECT * FROM products WHERE name = '__test_widget__'"
  );
  assert(pSel.rows.length === 1, "queried test product");
  await pgPool.query("DELETE FROM products WHERE name = '__test_widget__'");

  // --- Postgres: CRUD on orders (feature-orders specific) ---
  await pgPool.query("DELETE FROM orders WHERE product_name = '__test_order__'");
  const oIns = await pgPool.query(
    "INSERT INTO orders (product_name, quantity) VALUES ('__test_order__', 3) RETURNING *"
  );
  assert(oIns.rows.length === 1, "inserted test order");
  assert(oIns.rows[0].product_name === "__test_order__", "order product_name correct");
  assert(oIns.rows[0].quantity === 3, "order quantity correct");

  const oSel = await pgPool.query(
    "SELECT * FROM orders WHERE product_name = '__test_order__'"
  );
  assert(oSel.rows.length === 1, "queried test order");

  // Test default quantity
  const oDefault = await pgPool.query(
    "INSERT INTO orders (product_name) VALUES ('__test_default__') RETURNING *"
  );
  assert(oDefault.rows[0].quantity === 1, "orders default quantity is 1");

  await pgPool.query("DELETE FROM orders WHERE product_name LIKE '__test_%'");

  // --- Redis: write/read ---
  await redisClient.set("__test_key__", "hello_from_feature_orders");
  const val = await redisClient.get("__test_key__");
  assert(val === "hello_from_feature_orders", "redis write/read works");
  await redisClient.del("__test_key__");

  // --- Redis: isolation check ---
  const marker = await redisClient.get("instance_marker");
  assert(
    marker === null || marker === "feature-orders",
    "redis has no foreign instance marker (isolated)"
  );

  // --- Cleanup ---
  await redisClient.quit();
  await pgPool.end();

  console.log("");
  console.log(`${passed} passed, ${failed} failed`);
  process.exit(failed > 0 ? 1 : 0);
}

run().catch((err) => {
  console.error("Test error:", err);
  process.exit(1);
});
FEATURE_ORDERS_TEST_EOF
    git add server.js test.js
    git commit -m "feature: add orders table, endpoints, and tests"

    # Return to main
    git checkout main

    echo "  coast-demo ready (branches: main, feature-users, feature-orders)"
}

# --- coast-api ---
# A lightweight API gateway with Redis only (no Postgres).
# Different ports from coast-demo (34000 vs 33000).
# Tests multi-project tandem operation.

setup_coast_api() {
    local dir="$PROJECTS_DIR/coast-api"
    echo "Setting up coast-api..."

    # Clean any existing git state for idempotency
    rm -rf "$dir/.git" "$dir/docker-compose.override.yml"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: API gateway with Redis"

    # Feature branch: change the service message
    git checkout -b feature-v2
    cat > server.js << 'FEATURE_V2_EOF'
const http = require("http");
const { createClient } = require("redis");

const PORT = 3000;

const redisClient = createClient({ url: process.env.REDIS_URL });

async function init() {
  await redisClient.connect();
  console.log("Connected to Redis.");
}

const server = http.createServer(async (req, res) => {
  const json = (data, status = 200) => {
    res.writeHead(status, { "Content-Type": "application/json" });
    res.end(JSON.stringify(data));
  };

  try {
    if (req.url === "/health") {
      return json({ status: "ok" });
    }

    if (req.url === "/redis-info") {
      const marker = await redisClient.get("instance_marker");
      const counter = await redisClient.get("request_counter");
      return json({
        instance_marker: marker,
        request_counter: counter ? parseInt(counter) : 0,
      });
    }

    if (req.url === "/redis-set-marker") {
      await redisClient.set("instance_marker", "feature-v2");
      const count = await redisClient.incr("request_counter");
      return json({ marker: "feature-v2", request_counter: count });
    }

    // Default: status endpoint
    const count = await redisClient.incr("request_counter");
    return json({
      service: "coast-api",
      message: "API Gateway V2",
      branch: "feature-v2",
      request_count: count,
    });
  } catch (err) {
    console.error("Request error:", err);
    json({ error: err.message }, 500);
  }
});

init()
  .then(() => {
    server.listen(PORT, () => {
      console.log(`Coast API listening on :${PORT}`);
    });
  })
  .catch((err) => {
    console.error("Failed to start:", err);
    process.exit(1);
  });
FEATURE_V2_EOF
    git add server.js
    git commit -m "feature: v2 API gateway"

    # Return to main
    git checkout main

    echo "  coast-api ready (branches: main, feature-v2)"
}

# --- coast-secrets ---
# A minimal Node.js app for testing coast secret injection.
# No database or Redis — just an HTTP server that exposes injected secrets.

setup_coast_secrets() {
    local dir="$PROJECTS_DIR/coast-secrets"
    echo "Setting up coast-secrets..."

    # Clean any existing git state for idempotency
    rm -rf "$dir/.git" "$dir/docker-compose.override.yml"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: secrets test project"

    echo "  coast-secrets ready (branches: main)"
}

# --- coast-claude ---
# A demo showing Claude Code running inside a coast with the host's API key
# extracted from macOS Keychain and injected as ANTHROPIC_API_KEY.

setup_coast_claude() {
    local dir="$PROJECTS_DIR/coast-claude"
    echo "Setting up coast-claude..."

    # Clean any existing git state for idempotency
    rm -rf "$dir/.git" "$dir/docker-compose.override.yml"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: Claude Code coast demo"

    echo "  coast-claude ready (branches: main)"
}

# --- coast-benchmark ---
# A minimal Node.js HTTP server with zero dependencies (no npm install).
# Used to benchmark coast's scaling: build time, N instance spin-up, checkout swap.
# Each feature branch returns a unique JSON response identifying its branch name.

setup_coast_benchmark() {
    local dir="$PROJECTS_DIR/coast-benchmark"
    local count="${COAST_BENCHMARK_COUNT:-3}"
    echo "Setting up coast-benchmark (${count} feature branches)..."

    # Clean any existing git state for idempotency
    rm -rf "$dir/.git" "$dir/docker-compose.override.yml"

    cd "$dir"

    # Write the base server.js explicitly so setup is idempotent even if
    # a previous test's `coast assign` did `git checkout feature-XX` on the host.
    cat > server.js << 'BENCHMARK_MAIN_EOF'
const http = require("http");

const server = http.createServer((req, res) => {
  const json = (data) => {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify(data));
  };

  if (req.url === "/health") return json({ status: "ok" });
  return json({ service: "coast-benchmark", feature: "main" });
});

server.listen(3000, () => console.log("Benchmark server on :3000"));
BENCHMARK_MAIN_EOF

    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: benchmark server (main)"

    # Create feature-01 through feature-NN branches
    # Zero-padded names prevent substring collisions in assertions
    for n in $(seq 1 "$count"); do
        local padded
        padded=$(printf '%02d' "$n")
        git checkout -b "feature-$padded"
        sed -i '' "s/feature: \"main\"/feature: \"feature-$padded\"/" server.js
        git add server.js
        git commit -m "feature-$padded: return unique feature name"
        git checkout main
    done

    echo "  coast-benchmark ready (branches: main + feature-01..feature-$(printf '%02d' "$count"))"
}

# --- coast-egress ---
# A minimal Node.js app that reaches a host-machine service via egress.
# Tests that Coast's [egress] directive enables host connectivity from inner containers.

setup_coast_egress() {
    local dir="$PROJECTS_DIR/coast-egress"
    echo "Setting up coast-egress..."

    # Clean any existing git state for idempotency
    rm -rf "$dir/.git" "$dir/docker-compose.override.yml"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: egress test project"

    echo "  coast-egress ready (branches: main)"
}

# --- coast-volumes ---
# A minimal Node.js app with Postgres and Redis for testing volume strategies.
# Three Coastfile variants: shared, isolated, shared_services.
# Test scripts copy the appropriate variant before building.

setup_coast_volumes() {
    local dir="$PROJECTS_DIR/coast-volumes"
    echo "Setting up coast-volumes..."

    rm -rf "$dir/.git" "$dir/docker-compose.override.yml"

    cd "$dir"

    # Default Coastfile to shared strategy
    cp Coastfile.shared Coastfile

    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: volume strategy test project"

    echo "  coast-volumes ready (branches: main)"
}

# --- Run all setups ---
setup_coast_demo
setup_coast_api
setup_coast_secrets
setup_coast_claude
setup_coast_benchmark
setup_coast_egress
setup_coast_volumes

# --- coast-hmr ---
# A minimal Node.js server that re-reads data.json on every request.
# Uses a volume mount (no Dockerfile COPY) so file changes through the
# overlay are immediately visible — tests HMR-like hot-reload behaviour.

setup_coast_hmr() {
    local dir="$PROJECTS_DIR/coast-hmr"
    echo "Setting up coast-hmr..."

    rm -rf "$dir/.git" "$dir/docker-compose.override.yml"

    # Reset data.json to initial state (may have been modified by previous test)
    cat > "$dir/data.json" <<'DATAJSON'
{"message": "initial", "version": 1}
DATAJSON

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: HMR test project"

    echo "  coast-hmr ready (branches: main)"
}

setup_coast_hmr

# --- coast-mcp ---
# A minimal Node.js app with MCP server declarations.
# Tests that Coastfile [mcp.*] sections parse correctly and that
# internal MCP servers get installed at /mcp/<name>/ during coast build.

setup_coast_mcp() {
    local dir="$PROJECTS_DIR/coast-mcp"
    echo "Setting up coast-mcp..."

    rm -rf "$dir/.git" "$dir/docker-compose.override.yml"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: MCP test project"

    echo "  coast-mcp ready (branches: main)"
}

setup_coast_mcp

# --- coast-agent-shell ---
# A minimal project that tests the [agent_shell] Coastfile feature.
# The agent shell runs a heartbeat loop instead of a real agent binary.

setup_coast_agent_shell() {
    local dir="$PROJECTS_DIR/coast-agent-shell"
    echo "Setting up coast-agent-shell..."

    rm -rf "$dir/.git"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: agent shell test project"

    echo "  coast-agent-shell ready (branches: main)"
}

setup_coast_agent_shell

# --- coast-bare ---
# A Coast project using bare process services (no docker-compose).

setup_coast_bare() {
    local dir="$PROJECTS_DIR/coast-bare"
    echo "Setting up coast-bare..."
    mkdir -p "$dir"

    rm -rf "$dir/.git" "$dir/.coasts"

    cat > "$dir/Coastfile" << 'COASTFILE_EOF'
# coast-bare: A Coast project using bare process services.
#
# This demonstrates running plain processes (no Docker Compose)
# inside a coast DinD container. The [services] section defines
# commands that coast supervises with log capture and optional restarts.

[coast]
name = "coast-bare"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
command = "node server.js"
port = 40000
restart = "on-failure"

[ports]
web = 40000
COASTFILE_EOF

    cat > "$dir/server.js" << 'SERVERJS_EOF'
const http = require("http");
const os = require("os");

const PORT = process.env.PORT || 40000;

const server = http.createServer((req, res) => {
  res.writeHead(200, { "Content-Type": "application/json" });
  res.end(
    JSON.stringify({
      message: "Hello from Coast bare services!",
      hostname: os.hostname(),
      platform: os.platform(),
      uptime: process.uptime(),
    })
  );
});

server.listen(PORT, () => {
  console.log(`Server listening on port ${PORT}`);
});
SERVERJS_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: bare services with node server"

    # Feature branch with a different server response
    git checkout -b feature-v2

    cat > "$dir/server.js" << 'SERVERJS_V2_EOF'
const http = require("http");
const os = require("os");

const PORT = process.env.PORT || 40000;

const server = http.createServer((req, res) => {
  res.writeHead(200, { "Content-Type": "application/json" });
  res.end(
    JSON.stringify({
      message: "Hello from Coast bare services v2!",
      version: "2.0",
      hostname: os.hostname(),
      platform: os.platform(),
      uptime: process.uptime(),
    })
  );
});

server.listen(PORT, () => {
  console.log(`Server v2 listening on port ${PORT}`);
});
SERVERJS_V2_EOF

    git add -A
    git commit -m "feature: v2 server with version field"

    git checkout main
    echo "  coast-bare ready"
}

setup_coast_bare

# --- coast-simple ---
# A Coast project without docker-compose (purely for isolated DinD containers).

setup_coast_simple() {
    local dir="$PROJECTS_DIR/coast-simple"
    echo "Setting up coast-simple..."
    mkdir -p "$dir"

    cat > "$dir/Coastfile" << 'COASTFILE_EOF'
# coast-simple: A Coast project without docker-compose.
#
# This demonstrates using Coast purely for isolated DinD containers
# with tools installed via [coast.setup]. No compose file is needed.
# Use `coast exec` to run commands inside the container.

[coast]
name = "coast-simple"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

[ports]
app = 40000
COASTFILE_EOF

    cat > "$dir/server.js" << 'SERVERJS_EOF'
const http = require("http");
const os = require("os");

const PORT = process.env.PORT || 40000;

const server = http.createServer((req, res) => {
  res.writeHead(200, { "Content-Type": "application/json" });
  res.end(
    JSON.stringify({
      message: "Hello from Coast!",
      hostname: os.hostname(),
      platform: os.platform(),
      uptime: process.uptime(),
    })
  );
});

server.listen(PORT, () => {
  console.log(`Server listening on port ${PORT}`);
});
SERVERJS_EOF

    echo "  coast-simple ready (no git repo needed)"
}

setup_coast_simple

# --- coast-types ---
# Demonstrates composable Coastfile types with extends/includes/unset.

setup_coast_types() {
    local dir="$PROJECTS_DIR/coast-types"
    echo "Setting up coast-types..."
    mkdir -p "$dir"

    cat > "$dir/Coastfile" << 'COASTFILE_EOF'
# coast-types: Base Coastfile demonstrating composable types.
#
# This is the default configuration. Typed variants (Coastfile.light,
# Coastfile.shared) extend this using `extends = "Coastfile"`.
#
# Usage:
#   coast build                       # builds the default type
#   coast build --type light          # builds Coastfile.light
#   coast build --type shared         # builds Coastfile.shared
#   coast run dev-1                   # uses default build
#   coast run dev-2 --type light      # uses light build

[coast]
name = "coast-types"
runtime = "dind"

[coast.setup]
packages = ["curl", "jq"]
run = ["echo 'base setup complete'"]

[ports]
web = 38000
api = 38080
postgres = 35432
redis = 36379

[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"

[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]

[omit]
services = ["monitoring"]
COASTFILE_EOF

    cat > "$dir/Coastfile.light" << 'LIGHT_EOF'
# Coastfile.light — lightweight variant without shared services or heavy secrets.
#
# Inherits from the base Coastfile but strips out postgres, redis, and the
# db_password secret. Useful for frontend-only development or CI.

[coast]
extends = "Coastfile"

[coast.setup]
packages = ["nodejs"]
run = ["echo 'light setup appended'"]

[ports]
api = 39080

[unset]
secrets = ["db_password"]
shared_services = ["postgres", "redis"]
ports = ["postgres", "redis"]
LIGHT_EOF

    cat > "$dir/Coastfile.shared" << 'SHARED_EOF'
# Coastfile.shared — variant that adds extra shared services on top of base.
#
# Extends the base Coastfile, adds MongoDB, and includes an extra secrets
# fragment for demonstration.

[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]

[ports]
mongodb = 37017

[shared_services.mongodb]
image = "mongo:7"
ports = [27017]
env = { MONGO_INITDB_ROOT_USERNAME = "dev", MONGO_INITDB_ROOT_PASSWORD = "dev" }

[omit]
services = ["debug-tools"]
SHARED_EOF

    cat > "$dir/Coastfile.chain" << 'CHAIN_EOF'
# Coastfile.chain — demonstrates multi-level inheritance.
#
# Extends Coastfile.light (which itself extends Coastfile), forming
# a 3-level chain: Coastfile -> Coastfile.light -> Coastfile.chain.

[coast]
extends = "Coastfile.light"

[coast.setup]
run = ["echo 'chain setup appended'"]

[ports]
debug = 39999
CHAIN_EOF

    cat > "$dir/extra-secrets.toml" << 'SECRETS_EOF'
# Extra secrets fragment — included by Coastfile.shared.
#
# This file demonstrates the `includes` mechanism: it contributes
# secrets without needing a full Coastfile structure.

[coast]

[secrets.mongo_uri]
extractor = "env"
var = "MONGO_URI"
inject = "env:MONGO_URI"
SECRETS_EOF

    echo "  coast-types ready (no git repo needed)"
}

setup_coast_types

# --- host-shared-services-volume ---
# Tests that `coast shared-services rm` cleans up Docker volumes,
# so polluted volumes from other projects don't persist.

setup_host_shared_services_volume() {
    local dir="$PROJECTS_DIR/host-shared-services-volume"
    echo "Setting up host-shared-services-volume..."

    rm -rf "$dir/.git" "$dir/docker-compose.override.yml"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: shared services volume cleanup test"

    echo "  host-shared-services-volume ready (branches: main)"
}

setup_host_shared_services_volume

# --- coast-lookup ---
# A minimal Node.js server for testing `coast lookup`.
# Two feature branches so we can test worktree-based instance discovery.

setup_coast_lookup() {
    local dir="$PROJECTS_DIR/coast-lookup"
    echo "Setting up coast-lookup..."

    rm -rf "$dir/.git" "$dir/docker-compose.override.yml"

    cd "$dir"

    cat > server.js << 'LOOKUP_MAIN_EOF'
const http = require("http");

const server = http.createServer((req, res) => {
  const json = (data) => {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify(data));
  };

  if (req.url === "/health") return json({ status: "ok" });
  return json({ service: "coast-lookup", branch: "main" });
});

server.listen(3000, () => console.log("Lookup test server on :3000"));
LOOKUP_MAIN_EOF

    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: lookup test server (main)"

    git checkout -b feature-alpha
    sed -i '' 's/branch: "main"/branch: "feature-alpha"/' server.js
    git add server.js
    git commit -m "feature-alpha: return unique branch name"
    git checkout main

    git checkout -b feature-beta
    sed -i '' 's/branch: "main"/branch: "feature-beta"/' server.js
    git add server.js
    git commit -m "feature-beta: return unique branch name"
    git checkout main

    echo "  coast-lookup ready (branches: main, feature-alpha, feature-beta)"
}

setup_coast_lookup

# --- coast-dangling ---
# A minimal project for testing dangling container detection.
# Has a shared redis service so tests can cover both instance and shared-service danglers.

setup_coast_dangling() {
    local dir="$PROJECTS_DIR/coast-dangling"
    echo "Setting up coast-dangling..."

    rm -rf "$dir/.git"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: dangling container test project"

    echo "  coast-dangling ready (branches: main)"
}

setup_coast_dangling

echo ""
echo "All examples initialized. Run 'coast build' inside any example to get started."
