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

# --- coast-mixed ---
# A Coast project combining Docker Compose services with bare process services.
# Tests that compose + [services] coexist: the compose file runs an API server,
# while a bare service runs a simulated vite dev server on the DinD host.

setup_coast_mixed() {
    local dir="$PROJECTS_DIR/coast-mixed"
    echo "Setting up coast-mixed..."
    mkdir -p "$dir"

    rm -rf "$dir/.git" "$dir/.coasts"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: mixed compose + bare services"

    git checkout -b feature-v2

    cat > "$dir/server.js" << 'SERVERJS_V2_EOF'
const http = require("http");
const os = require("os");

const PORT = 3000;

const server = http.createServer((req, res) => {
  const json = (data, status = 200) => {
    res.writeHead(status, { "Content-Type": "application/json" });
    res.end(JSON.stringify(data));
  };

  if (req.url === "/health") {
    return json({ status: "ok" });
  }

  return json({
    service: "api",
    version: "v2",
    message: "Hello from Coast mixed services v2 (compose API)!",
    hostname: os.hostname(),
    uptime: process.uptime(),
  });
});

server.listen(PORT, () => {
  console.log(`API server v2 listening on port ${PORT}`);
});
SERVERJS_V2_EOF

    cat > "$dir/vite-server.js" << 'VITEJS_V2_EOF'
const http = require("http");
const os = require("os");

const PORT = 40100;

const server = http.createServer((req, res) => {
  const json = (data, status = 200) => {
    res.writeHead(status, { "Content-Type": "application/json" });
    res.end(JSON.stringify(data));
  };

  if (req.url === "/health") {
    return json({ status: "ok" });
  }

  return json({
    service: "vite",
    version: "v2",
    message: "Hello from Coast mixed services v2 (bare vite)!",
    hostname: os.hostname(),
    uptime: process.uptime(),
  });
});

server.listen(PORT, () => {
  console.log(`Vite dev server v2 listening on port ${PORT}`);
});
VITEJS_V2_EOF

    git add -A
    git commit -m "feature: v2 responses for both compose and bare services"

    git checkout main
    echo "  coast-mixed ready (branches: main, feature-v2)"
}

setup_coast_mixed

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

# --- coast-reboot-recovery ---
# Project that exercises post-reboot recovery. Mirrors the shape of the
# real-world project that hit this: compose app with env_file references
# under /workspace talking to shared postgres and redis on the host
# Docker daemon. Used by test_reboot_recovery.sh.

setup_coast_reboot_recovery() {
    local dir="$PROJECTS_DIR/coast-reboot-recovery"
    echo "Setting up coast-reboot-recovery..."

    rm -rf "$dir/.git" "$dir/docker-compose.override.yml"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    # Force-add app/.env (typically .env is gitignored elsewhere, but for this
    # fixture it must be committed so `coast run` sees it on rebind).
    git add -A -f
    git commit -m "initial commit: reboot recovery test project"

    echo "  coast-reboot-recovery ready (branch: main)"
}

setup_coast_reboot_recovery

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

# --- coast-noautostart ---
# A minimal compose project with autostart = false.
# Used by test_restart_services.sh to verify down-only behavior.

setup_coast_noautostart() {
    local dir="$PROJECTS_DIR/coast-noautostart"
    echo "Setting up coast-noautostart..."

    rm -rf "$dir/.git"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: noautostart test project"

    echo "  coast-noautostart ready (branches: main)"
}

setup_coast_noautostart

# --- coast-private-paths ---
# Tests per-instance filesystem isolation via private_paths.
# No git repo needed, no compose, no services — just a DinD container
# with private_paths = ["data"] for mount isolation testing.

setup_coast_private_paths() {
    local dir="$PROJECTS_DIR/coast-private-paths"
    echo "Setting up coast-private-paths..."
    mkdir -p "$dir"

    cat > "$dir/Coastfile" << 'COASTFILE_EOF'
# coast-private-paths: Tests per-instance filesystem isolation via private_paths.
#
# The private_paths field gives each Coast instance its own bind mount
# for the listed directories, so writes (including flock locks) don't
# conflict across instances sharing the same host project root.

[coast]
name = "coast-private-paths"
runtime = "dind"
private_paths = ["data"]
autostart = false
COASTFILE_EOF

    echo "  coast-private-paths ready"
}

setup_coast_private_paths

# --- coast-private-paths-bare ---
# Combines private_paths with bare services and assign to test that
# private_paths overlays survive assign/unassign and bare services restart.

setup_coast_private_paths_bare() {
    local dir="$PROJECTS_DIR/coast-private-paths-bare"
    echo "Setting up coast-private-paths-bare..."
    mkdir -p "$dir"

    rm -rf "$dir/.git" "$dir/.worktrees"

    cat > "$dir/Coastfile" << 'COASTFILE_EOF'
[coast]
name = "coast-private-paths-bare"
runtime = "dind"
private_paths = ["data"]

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = ["echo 'install step 1' >> /var/log/coast-services/web.install.log", "sleep 15 && echo 'install step 2 (slow)' >> /var/log/coast-services/web.install.log"]
command = "node server.js"
port = 41000
restart = "on-failure"

[ports]
web = 41000

[assign]
default = "none"

[assign.services]
web = "restart"
COASTFILE_EOF

    cat > "$dir/server.js" << 'SERVERJS_EOF'
const http = require("http");
const fs = require("fs");
const { spawn, execSync } = require("child_process");
const PORT = process.env.PORT || 41000;
const LOCK_PATH = "/workspace/data/app.lock";

// Acquire a persistent flock at startup, mimicking Next.js .next/trace lock.
// Spawns a background `flock -x <file> -c "sleep infinity"` that holds the
// lock for the entire process lifetime. If the old server wasn't killed or
// the private_paths overlay leaked, flock -n will fail.
let lockHeld = false;
let lockChild = null;
try {
  fs.mkdirSync("/workspace/data", { recursive: true });
  // Non-blocking flock test: exit 0 if acquired, exit 1 if held
  execSync(`flock -n "${LOCK_PATH}" true`, { stdio: "ignore" });
  // If we get here, lock is available. Hold it persistently.
  lockChild = spawn("flock", ["-x", LOCK_PATH, "-c", "sleep infinity"], {
    stdio: "ignore", detached: false
  });
  lockHeld = true;
  console.log("Acquired exclusive flock on " + LOCK_PATH);
} catch (e) {
  console.error("FLOCK FAILED: Could not acquire lock on " + LOCK_PATH);
  console.error("This means a stale lock leaked across the workspace remount.");
  // Server starts but reports lock failure
}

process.on("exit", () => { if (lockChild) lockChild.kill(); });

const server = http.createServer((req, res) => {
  if (req.url === "/health") {
    res.writeHead(200); res.end("ok"); return;
  }
  if (req.url === "/lock-status") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ lock_held: lockHeld, lock_path: LOCK_PATH }));
    return;
  }
  res.writeHead(200, { "Content-Type": "application/json" });
  res.end(JSON.stringify({ version: "main", branch: "main", lock_held: lockHeld }));
});

server.listen(PORT, () => console.log("main server on " + PORT));
SERVERJS_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: private-paths-bare main"

    git checkout -b feature-v2

    cat > "$dir/server.js" << 'SERVERJS_V2_EOF'
const http = require("http");
const fs = require("fs");
const { spawn, execSync } = require("child_process");
const PORT = process.env.PORT || 41000;
const LOCK_PATH = "/workspace/data/app.lock";

let lockHeld = false;
let lockChild = null;
try {
  fs.mkdirSync("/workspace/data", { recursive: true });
  execSync(`flock -n "${LOCK_PATH}" true`, { stdio: "ignore" });
  lockChild = spawn("flock", ["-x", LOCK_PATH, "-c", "sleep infinity"], {
    stdio: "ignore", detached: false
  });
  lockHeld = true;
  console.log("Acquired exclusive flock on " + LOCK_PATH);
} catch (e) {
  console.error("FLOCK FAILED: Could not acquire lock on " + LOCK_PATH);
  console.error("This means a stale lock leaked across the workspace remount.");
}

process.on("exit", () => { if (lockChild) lockChild.kill(); });

const server = http.createServer((req, res) => {
  if (req.url === "/health") {
    res.writeHead(200); res.end("ok"); return;
  }
  if (req.url === "/lock-status") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ lock_held: lockHeld, lock_path: LOCK_PATH }));
    return;
  }
  res.writeHead(200, { "Content-Type": "application/json" });
  res.end(JSON.stringify({ version: "v2", branch: "feature-v2", lock_held: lockHeld }));
});

server.listen(PORT, () => console.log("v2 server on " + PORT));
SERVERJS_V2_EOF

    git add -A
    git commit -m "feature: v2 server"

    git checkout main
    echo "  coast-private-paths-bare ready (branches: main, feature-v2)"
}

setup_coast_private_paths_bare

# --- coast-remote ---
# Minimal project for remote coast integration testing.
# Uses a bare Node.js service with a Coastfile.remote.toml variant.

setup_coast_remote() {
    local dir="$PROJECTS_DIR/remote/coast-remote-basic"
    echo "Setting up coast-remote-basic..."

    rm -rf "$dir/.git"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: remote coast test project"

    git checkout -b feature-sync-test
    sed -i 's/Hello from Remote Coast!/Hello from feature branch!/' server.js
    git add -A
    git commit -m "feature: change greeting for sync test"
    git checkout main

    echo "  coast-remote-basic ready (branches: main, feature-sync-test)"
}

setup_coast_remote

# --- coast-remote-compose-build ---
setup_coast_remote_compose_build() {
    local dir="$PROJECTS_DIR/remote/coast-remote-compose-build"
    echo "Setting up coast-remote-compose-build..."

    rm -rf "$dir/.git"

    cd "$dir"
    git init -b main
    git add -A
    git commit -m "initial commit: remote compose build test project"

    echo "  coast-remote-compose-build ready (branch: main)"
}

setup_coast_remote_compose_build

# --- coast-remote-assign ---
# Project for testing remote assign with external worktrees and compose services.
# Has two branches (main, feature-assign-test) and an external worktree.

setup_coast_remote_assign() {
    local dir="$PROJECTS_DIR/remote/coast-remote-assign"
    echo "Setting up coast-remote-assign..."

    rm -rf "$dir/.git"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: remote assign test project"

    # Add a main-only marker file (deleted on feature branch)
    echo "this file only exists on main" > MAIN_ONLY_MARKER.txt
    git add -A
    git commit --amend --no-edit

    git checkout -b feature-assign-test
    sed -i 's/Hello from main branch!/Hello from feature branch!/' app/server.js
    rm -f MAIN_ONLY_MARKER.txt
    git add -A
    git commit -m "feature: change greeting for assign test"
    git checkout main

    # Create external worktree (simulates ~/conductor/workspaces/... pattern)
    mkdir -p /tmp/coast-assign-worktrees
    git worktree add /tmp/coast-assign-worktrees/feature-assign-test feature-assign-test 2>/dev/null || true

    echo "  coast-remote-assign ready (branches: main, feature-assign-test; external worktree at /tmp/coast-assign-worktrees/)"
}

setup_coast_remote_assign

# --- coast-remote-hot ---
# Project for testing hot-reload assign strategy on remote coasts.
# Server re-reads data.json on every request. Feature branch changes data.json.

setup_coast_remote_hot() {
    local dir="$PROJECTS_DIR/remote/coast-remote-hot"
    echo "Setting up coast-remote-hot..."

    rm -rf "$dir/.git"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: remote hot test project"

    git checkout -b feature-hot-test
    printf '{"message": "feature-data", "version": 2}\n' > data.json
    git add -A
    git commit -m "feature: update data.json for hot test"
    git checkout main

    mkdir -p .worktrees
    git worktree add .worktrees/feature-hot-test feature-hot-test 2>/dev/null || true

    echo "  coast-remote-hot ready (branches: main, feature-hot-test)"
}

setup_coast_remote_hot

# --- coast-remote-rebuild ---
# Project for testing rebuild assign strategy on remote coasts.
# Feature branch changes version.txt (a rebuild trigger), forcing image rebuild.

setup_coast_remote_rebuild() {
    local dir="$PROJECTS_DIR/remote/coast-remote-rebuild"
    echo "Setting up coast-remote-rebuild-test..."

    rm -rf "$dir/.git"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: remote rebuild test project"

    git checkout -b feature-rebuild-test
    echo "v2-feature" > app/version.txt
    git add -A
    git commit -m "feature: bump version for rebuild test"
    git checkout main

    mkdir -p .worktrees
    git worktree add .worktrees/feature-rebuild-test feature-rebuild-test 2>/dev/null || true

    echo "  coast-remote-rebuild ready (branches: main, feature-rebuild-test)"
}

setup_coast_remote_rebuild

# --- coast-remote-file-watcher ---
setup_coast_remote_file_watcher() {
    local dir="$PROJECTS_DIR/remote/coast-remote-file-watcher"
    echo "Setting up coast-remote-file-watcher..."

    rm -rf "$dir/.git"

    cd "$dir"
    chmod +x watcher.sh
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: file watcher race test project"

    git checkout -b feature-watcher-test
    sed -i 's/Hello from main branch!/Hello from feature branch!/' server.js
    git add -A
    git commit -m "feature: change greeting for watcher race test"
    git checkout main

    echo "  coast-remote-file-watcher ready (branches: main, feature-watcher-test)"
}

setup_coast_remote_file_watcher

# --- coast-remote-compose ---
# Project for testing inner compose service healing.
# Has a "fragile-cache" service without restart policy.

setup_coast_remote_compose() {
    local dir="$PROJECTS_DIR/remote/coast-remote-compose"
    echo "Setting up coast-remote-compose..."

    rm -rf "$dir/.git"

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: compose service healing test project"

    echo "  coast-remote-compose ready"
}

setup_coast_remote_compose

# --- coast-remote-stale-test ---
setup_coast_remote_stale_test() {
    local dir="$PROJECTS_DIR/remote/coast-remote-stale-test"
    echo "Setting up coast-remote-stale-test..."
    rm -rf "$dir/.git"
    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: stale sshd recovery test project"
    echo "  coast-remote-stale-test ready"
}

setup_coast_remote_stale_test

# --- coast-no-coastfile ---
setup_coast_no_coastfile() {
    local dir="$PROJECTS_DIR/coast-no-coastfile"
    echo "Setting up coast-no-coastfile..."
    rm -rf "$dir/.git"
    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: coastfile-less build test project"
    echo "  coast-no-coastfile ready"
}

setup_coast_no_coastfile

# --- coast-envvar ---
setup_coast_envvar() {
    local dir="$PROJECTS_DIR/coast-envvar"
    echo "Setting up coast-envvar..."
    rm -rf "$dir/.git"
    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: env var interpolation test project"
    echo "  coast-envvar ready"
}

setup_coast_envvar

# --- coast-working-dir ---
setup_coast_working_dir() {
    local dir="$PROJECTS_DIR/coast-working-dir"
    echo "Setting up coast-working-dir..."
    rm -rf "$dir/.git"
    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: working-dir flag test project"
    echo "  coast-working-dir ready"
}

setup_coast_working_dir

# --- coast-ssg-minimal ---
# Minimal Shared Service Group: one postgres service with `*-alpine`
# image for fast pulls in CI. Declares an inner named volume (pg_data)
# so Phase 3's `test_ssg_named_volume_persists.sh` has data to write
# into and verify across stop/start. No host bind mounts: the test
# doesn't depend on external filesystem state.
# Used by `test_ssg_build_minimal.sh` and
# `test_ssg_named_volume_persists.sh`.

setup_coast_ssg_minimal() {
    local dir="$PROJECTS_DIR/coast-ssg-minimal"
    echo "Setting up coast-ssg-minimal..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile.shared_service_groups" << 'SSG_MINIMAL_EOF'
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["pg_data:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "coast" }
SSG_MINIMAL_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: minimal SSG with one postgres service"
    echo "  coast-ssg-minimal ready"
}

setup_coast_ssg_minimal

# --- coast-ssg-multi-service ---
# Multi-service SSG: postgres + redis, both `*-alpine`. Used by
# `test_ssg_build_multiple_services.sh` and
# `test_ssg_build_rebuild_prunes.sh`.

setup_coast_ssg_multi_service() {
    local dir="$PROJECTS_DIR/coast-ssg-multi-service"
    echo "Setting up coast-ssg-multi-service..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile.shared_service_groups" << 'SSG_MULTI_EOF'
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_PASSWORD = "coast" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
SSG_MULTI_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: multi-service SSG (postgres + redis)"
    echo "  coast-ssg-multi-service ready"
}

setup_coast_ssg_multi_service

# --- coast-ssg-bind-mount ---
# SSG with a host bind mount. Used by Phase 3's
# `test_ssg_bind_mount_symmetric.sh` to verify that the same host
# directory is visible with the same inodes inside the outer DinD and
# inside the inner postgres container (the symmetric-path plan in
# `coast-ssg/DESIGN.md §10.2`).
#
# The host source lives under `$COAST_SSG_BIND_HOST_ROOT`. When the
# env var is unset we default to `/root/coast-ssg-bind-mount/pg-data`
# which is reachable through the dindind test container's persistent
# volume tree. Under docker-in-docker, `/tmp` is backed by a tmpfs
# that the outer daemon cannot bind-mount through into a nested
# container, so `/tmp` does NOT work here — the chosen path must live
# on the same filesystem as the dindind daemon's workdir.
#
# The test creates `$COAST_SSG_BIND_HOST_ROOT` before calling
# `coast ssg run`; this project only declares the mount shape.

setup_coast_ssg_bind_mount() {
    local dir="$PROJECTS_DIR/coast-ssg-bind-mount"
    local host_root="${COAST_SSG_BIND_HOST_ROOT:-/root/coast-ssg-bind-mount}"
    echo "Setting up coast-ssg-bind-mount (host_root=$host_root)..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile.shared_service_groups" <<SSG_BIND_EOF
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["${host_root}/pg-data:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "coast" }
SSG_BIND_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: SSG with host bind mount"
    echo "  coast-ssg-bind-mount ready"
}

setup_coast_ssg_bind_mount

# --- coast-ssg-consumer ---
# Consumer coast that declares a `from_group = true` reference to the
# SSG postgres service but is otherwise a no-op DinD box. Used by
# Phase 3.5's `test_ssg_auto_start_on_run.sh` to verify that
# `coast run` auto-starts the SSG on behalf of a consumer that
# references it.
#
# Intentionally does NOT include any inner compose service, so
# `coast run` can complete successfully even though the SSG->consumer
# routing hasn't shipped yet (Phase 4). The test only cares that the
# singleton `coast-ssg` container is up after `coast run` returns.

setup_coast_ssg_consumer() {
    local dir="$PROJECTS_DIR/coast-ssg-consumer"
    echo "Setting up coast-ssg-consumer..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile" << 'SSG_CONSUMER_EOF'
# coast-ssg-consumer: a regular coast that opts into the Shared Service
# Group via `from_group = true`. Phase 3.5 auto-starts the SSG as part
# of `coast run`. Phase 4 will wire the actual routing.

[coast]
name = "coast-ssg-consumer"
runtime = "dind"

[shared_services.postgres]
from_group = true
SSG_CONSUMER_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: consumer that references SSG postgres"
    echo "  coast-ssg-consumer ready"
}

setup_coast_ssg_consumer

# --- coast-ssg-consumer-basic ---
# Phase 4 consumer with a real inner compose. The app service runs a
# long-sleeping postgres:16-alpine container (same image as the SSG
# service — reused to avoid an extra image pull) so the test can
# `docker exec <instance-app-container> psql -h postgres ...` and
# prove end-to-end connectivity through:
#
#     consumer app -> postgres:5432 (DNS via compose_rewrite extra_hosts)
#       -> docker0 alias IP (socat listener)
#       -> host.docker.internal:<ssg-dynamic-port> (SSG's outer publish)
#       -> inner SSG postgres
#
# Reused by `test_ssg_consumer_basic.sh` and `test_ssg_port_collision.sh`.

setup_coast_ssg_consumer_basic() {
    local dir="$PROJECTS_DIR/coast-ssg-consumer-basic"
    echo "Setting up coast-ssg-consumer-basic..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile" << 'SSG_BASIC_COASTFILE_EOF'
# coast-ssg-consumer-basic: a consumer with a real inner compose that
# actually connects to the SSG postgres. Phase 4.

[coast]
name = "coast-ssg-consumer-basic"
compose = "./docker-compose.yml"
runtime = "dind"

[shared_services.postgres]
from_group = true
SSG_BASIC_COASTFILE_EOF

    cat > "$dir/docker-compose.yml" << 'SSG_BASIC_COMPOSE_EOF'
services:
  app:
    image: postgres:16-alpine
    command: ["sh", "-c", "while true; do sleep 10; done"]
    environment:
      PGPASSWORD: "coast"
SSG_BASIC_COMPOSE_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: consumer with real compose + SSG postgres reference"
    echo "  coast-ssg-consumer-basic ready"
}

setup_coast_ssg_consumer_basic

# --- coast-ssg-consumer-conflict-forbidden ---
# Phase 1 conflict: `from_group = true` with forbidden inline fields.
# `coast build` must reject this at parse time. No runtime needed.

setup_coast_ssg_consumer_conflict_forbidden() {
    local dir="$PROJECTS_DIR/coast-ssg-consumer-conflict-forbidden"
    echo "Setting up coast-ssg-consumer-conflict-forbidden..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile" << 'SSG_CONFLICT_FORBIDDEN_EOF'
[coast]
name = "coast-ssg-consumer-conflict-forbidden"
runtime = "dind"

[shared_services.postgres]
from_group = true
image = "postgres:16-alpine"
SSG_CONFLICT_FORBIDDEN_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: from_group = true with forbidden image field"
    echo "  coast-ssg-consumer-conflict-forbidden ready"
}

setup_coast_ssg_consumer_conflict_forbidden

# --- coast-ssg-consumer-conflict-duplicate ---
# TOML-level conflict: two `[shared_services.postgres]` header blocks.
# TOML parsers reject duplicate keys, so `coast build` surfaces a
# parse error before it even reaches the from_group validation.

setup_coast_ssg_consumer_conflict_duplicate() {
    local dir="$PROJECTS_DIR/coast-ssg-consumer-conflict-duplicate"
    echo "Setting up coast-ssg-consumer-conflict-duplicate..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile" << 'SSG_CONFLICT_DUP_EOF'
[coast]
name = "coast-ssg-consumer-conflict-duplicate"
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]

[shared_services.postgres]
from_group = true
SSG_CONFLICT_DUP_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: duplicate shared_services.postgres sections"
    echo "  coast-ssg-consumer-conflict-duplicate ready"
}

setup_coast_ssg_consumer_conflict_duplicate

# --- coast-ssg-consumer-missing ---
# Consumer references an SSG service name that doesn't exist in the
# active SSG build. `coast build` succeeds (build doesn't cross-check
# the SSG — Phase 7 adds drift detection). `coast run` must fail fast
# with the DESIGN.md §6.1 "missing service" wording.

setup_coast_ssg_consumer_missing() {
    local dir="$PROJECTS_DIR/coast-ssg-consumer-missing"
    echo "Setting up coast-ssg-consumer-missing..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile" << 'SSG_MISSING_EOF'
[coast]
name = "coast-ssg-consumer-missing"
runtime = "dind"

[shared_services.nonexistent_svc]
from_group = true
SSG_MISSING_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: consumer referencing nonexistent SSG service"
    echo "  coast-ssg-consumer-missing ready"
}

setup_coast_ssg_consumer_missing

# --- coast-ssg-consumer-remote ---
# Phase 4.5 consumer that builds+runs on a remote coast-service and
# consumes the LOCAL SSG via reverse SSH tunnels (DESIGN.md §20).
#
# The inner app container runs `postgres:16-alpine` (so psql is
# available) and loops forever; the integration test execs psql
# against `postgres:5432` through the tunnel chain.
#
# Two Coastfile variants:
#   Coastfile              - base (local build works too)
#   Coastfile.remote.toml  - extends base + adds [remote] section

setup_coast_ssg_consumer_remote() {
    local dir="$PROJECTS_DIR/coast-ssg-consumer-remote"
    echo "Setting up coast-ssg-consumer-remote..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile" << 'SSG_REMOTE_COASTFILE_EOF'
# coast-ssg-consumer-remote: Phase 4.5 remote consumer.
# Base variant; the remote build uses `Coastfile.remote.toml`.

[coast]
name = "coast-ssg-consumer-remote"
compose = "./docker-compose.yml"
runtime = "dind"

[shared_services.postgres]
from_group = true
SSG_REMOTE_COASTFILE_EOF

    cat > "$dir/Coastfile.remote.toml" << 'SSG_REMOTE_REMOTE_EOF'
# Remote variant: extends the base Coastfile and declares [remote].

[coast]
extends = "Coastfile"

[remote]
SSG_REMOTE_REMOTE_EOF

    cat > "$dir/docker-compose.yml" << 'SSG_REMOTE_COMPOSE_EOF'
services:
  app:
    image: postgres:16-alpine
    command: ["sh", "-c", "while true; do sleep 10; done"]
    environment:
      PGPASSWORD: "coast"
SSG_REMOTE_COMPOSE_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: remote consumer that references SSG postgres"
    echo "  coast-ssg-consumer-remote ready"
}

setup_coast_ssg_consumer_remote

# --- coast-ssg-auto-db ---
# Phase 5 SSG fixture with `auto_create_db = true` on postgres.
# Separate from coast-ssg-minimal so Phase 2-4.5 tests keep their
# original behavior (no per-instance DB creation, no password pinning).
# POSTGRES_PASSWORD is pinned to `dev` to match the hardcoded
# `postgres:dev@...` credential baked into
# `coast_docker::compose::build_connection_url` used for `inject`.

setup_coast_ssg_auto_db() {
    local dir="$PROJECTS_DIR/coast-ssg-auto-db"
    echo "Setting up coast-ssg-auto-db..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile.shared_service_groups" << 'SSG_AUTODB_EOF'
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["pg_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "dev" }
auto_create_db = true
SSG_AUTODB_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: SSG with auto_create_db postgres"
    echo "  coast-ssg-auto-db ready"
}

setup_coast_ssg_auto_db

# --- coast-ssg-consumer-auto-db ---
# Consumer that references coast-ssg-auto-db's postgres via
# `from_group = true` and declares `inject = "env:DATABASE_URL"`.
# The app container is `postgres:16-alpine` (for `psql`) sleeping
# forever; the test execs psql inside it to query the DB through
# the normal routing chain.

setup_coast_ssg_consumer_auto_db() {
    local dir="$PROJECTS_DIR/coast-ssg-consumer-auto-db"
    echo "Setting up coast-ssg-consumer-auto-db..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile" << 'SSG_CONSUMER_AUTODB_COAST_EOF'
[coast]
name = "coast-ssg-consumer-auto-db"
compose = "./docker-compose.yml"
runtime = "dind"

[shared_services.postgres]
from_group = true
inject = "env:DATABASE_URL"
SSG_CONSUMER_AUTODB_COAST_EOF

    cat > "$dir/docker-compose.yml" << 'SSG_CONSUMER_AUTODB_COMPOSE_EOF'
services:
  app:
    image: postgres:16-alpine
    command: ["sh", "-c", "while true; do sleep 10; done"]
SSG_CONSUMER_AUTODB_COMPOSE_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: consumer with inject env:DATABASE_URL"
    echo "  coast-ssg-consumer-auto-db ready"
}

setup_coast_ssg_consumer_auto_db

# --- coast-shared-service-auto-db ---
# INLINE shared-service variant (no SSG). Proves the Phase 5 wiring
# also lights up auto_create_db + inject for `[shared_services.*]`
# declared directly on the consumer — DESIGN.md §13 claimed this
# already worked before Phase 5 but the runtime was never implemented;
# see DESIGN.md §17-20.

setup_coast_shared_service_auto_db() {
    local dir="$PROJECTS_DIR/coast-shared-service-auto-db"
    echo "Setting up coast-shared-service-auto-db..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile" << 'INLINE_AUTODB_COAST_EOF'
[coast]
name = "coast-shared-service-auto-db"
compose = "./docker-compose.yml"
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "dev" }
auto_create_db = true
inject = "env:DATABASE_URL"
INLINE_AUTODB_COAST_EOF

    cat > "$dir/docker-compose.yml" << 'INLINE_AUTODB_COMPOSE_EOF'
services:
  app:
    image: postgres:16-alpine
    command: ["sh", "-c", "while true; do sleep 10; done"]
INLINE_AUTODB_COMPOSE_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: inline shared postgres with auto_create_db + inject"
    echo "  coast-shared-service-auto-db ready"
}

setup_coast_shared_service_auto_db

# --- coast-canonical-5432-app ---
# Phase 6 fixture: a minimal coast that declares canonical port 5432
# for its own inner app. Used by the displacement test: after
# `coast run inst-a` + `coast checkout inst-a`, the daemon owns a
# socat on localhost:5432 forwarding to this coast's dynamic port.
# `coast ssg checkout postgres` then has to displace that socat.
# The app is postgres:16-alpine with a marker row we can probe to
# verify which data we're actually talking to.

setup_coast_canonical_5432_app() {
    local dir="$PROJECTS_DIR/coast-canonical-5432-app"
    echo "Setting up coast-canonical-5432-app..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile" << 'CANON_5432_COASTFILE_EOF'
[coast]
name = "coast-canonical-5432-app"
compose = "./docker-compose.yml"
runtime = "dind"

[ports]
db = 5432
CANON_5432_COASTFILE_EOF

    cat > "$dir/docker-compose.yml" << 'CANON_5432_COMPOSE_EOF'
services:
  db:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: dev
      POSTGRES_DB: postgres
    ports:
      - "5432:5432"
CANON_5432_COMPOSE_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: coast that owns canonical port 5432"
    echo "  coast-canonical-5432-app ready"
}

setup_coast_canonical_5432_app

# --- coast-ssg-consumer-multi ---
# Phase 7 fixture: a consumer coast referencing BOTH postgres and
# redis from the SSG. Used by the drift-missing-service test to
# simulate "SSG was rebuilt without redis, but the consumer still
# points at it". The app image isn't important — the drift check
# fires before any compose-up, so we just use alpine:3 as a
# placeholder that won't try to start anything that needs SSG.

setup_coast_ssg_consumer_multi() {
    local dir="$PROJECTS_DIR/coast-ssg-consumer-multi"
    echo "Setting up coast-ssg-consumer-multi..."
    mkdir -p "$dir"
    rm -rf "$dir/.git"

    cat > "$dir/Coastfile" << 'CONS_MULTI_COASTFILE_EOF'
[coast]
name = "coast-ssg-consumer-multi"
compose = "./docker-compose.yml"
runtime = "dind"

[shared_services.postgres]
from_group = true

[shared_services.redis]
from_group = true
CONS_MULTI_COASTFILE_EOF

    cat > "$dir/docker-compose.yml" << 'CONS_MULTI_COMPOSE_EOF'
services:
  app:
    image: alpine:3
    command: ["sh", "-c", "while true; do sleep 10; done"]
CONS_MULTI_COMPOSE_EOF

    cd "$dir"
    git init -b main
    git config user.name "Coast Dev"
    git config user.email "dev@coasts.dev"
    git add -A
    git commit -m "initial commit: consumer referencing postgres + redis from SSG"
    echo "  coast-ssg-consumer-multi ready"
}

setup_coast_ssg_consumer_multi

echo ""
echo "All examples initialized. Run 'coast build' inside any example to get started."
