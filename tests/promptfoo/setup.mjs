#!/usr/bin/env node

/**
 * MultiClaw PromptFoo Test Setup
 *
 * Prepares a fresh holding for PromptFoo evaluation:
 *   1. Wipes existing database state (drops and recreates tables)
 *   2. Stops all existing OpenClaw containers
 *   3. Initializes a new holding via POST /v1/install/init
 *   4. Waits for the MAIN agent to boot and become responsive
 *   5. Waits for MAIN to hire at least one CEO (organic behavior)
 *   6. Optionally waits for the CEO to hire managers
 *
 * Usage:
 *   node setup.mjs                    # Full setup: init + wait for CEO
 *   node setup.mjs --quick            # Just init, don't wait for org tree
 *   node setup.mjs --teardown         # Cleanup only (stop containers, wipe DB)
 *   node setup.mjs --status           # Show current holding status
 *
 * Environment variables:
 *   MULTICLAW_URL    Control plane URL (default: http://localhost:8080)
 *   MULTICLAW_DB_URL Postgres connection string (default: postgresql://multiclaw:multiclaw_pass@localhost:5432/multiclaw)
 */

const BASE_URL = process.env.MULTICLAW_URL || 'http://localhost:8080';
const DB_URL = process.env.MULTICLAW_DB_URL || 'postgresql://multiclaw:multiclaw_pass@localhost:5432/multiclaw';

// How long to wait for various stages (seconds)
const MAIN_BOOT_TIMEOUT = 180;
const CEO_HIRE_TIMEOUT = 300;
const MANAGER_HIRE_TIMEOUT = 300;
const POLL_INTERVAL = 5;

async function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

async function fetchJson(url, options = {}) {
  const res = await fetch(url, {
    headers: { 'Content-Type': 'application/json', ...options.headers },
    ...options,
  });
  const text = await res.text();
  try {
    return { status: res.status, data: JSON.parse(text) };
  } catch {
    return { status: res.status, data: { raw: text } };
  }
}

// ═══════════════════════════════════════════════════════════════
// Database Operations
// ═══════════════════════════════════════════════════════════════

/**
 * Wipe all data from the database. Uses psql directly since we can't
 * import pg in a zero-dependency script.
 */
async function wipeDatabase() {
  const { execSync } = await import('child_process');

  console.log('  Wiping database...');

  // Parse DB URL for psql
  const url = new URL(DB_URL);
  const env = {
    ...process.env,
    PGHOST: url.hostname,
    PGPORT: url.port || '5432',
    PGUSER: url.username,
    PGPASSWORD: url.password,
    PGDATABASE: url.pathname.slice(1),
  };

  const sql = `
    DO $$ DECLARE r RECORD;
    BEGIN
      FOR r IN (SELECT tablename FROM pg_tables WHERE schemaname = 'public') LOOP
        EXECUTE 'TRUNCATE TABLE ' || quote_ident(r.tablename) || ' CASCADE';
      END LOOP;
    END $$;
  `;

  try {
    execSync(`psql -c "${sql.replace(/"/g, '\\"')}"`, { env, stdio: 'pipe' });
    console.log('  Database wiped.');
  } catch (err) {
    console.error('  Failed to wipe database:', err.message);
    console.error('  Make sure psql is installed and the database is reachable.');
    process.exit(1);
  }
}

// ═══════════════════════════════════════════════════════════════
// Container Operations
// ═══════════════════════════════════════════════════════════════

async function stopAllOpenClawContainers() {
  const { execSync } = await import('child_process');

  console.log('  Stopping OpenClaw containers...');
  try {
    const containers = execSync(
      'docker ps -a --filter "name=openclaw-" --format "{{.Names}}"',
      { encoding: 'utf-8' }
    ).trim();

    if (containers) {
      const names = containers.split('\n').filter(Boolean);
      console.log(`  Found ${names.length} OpenClaw container(s) to stop.`);
      execSync(`docker rm -f ${names.join(' ')}`, { stdio: 'pipe' });
      console.log('  Containers stopped.');
    } else {
      console.log('  No OpenClaw containers running.');
    }
  } catch (err) {
    console.warn('  Warning: could not stop containers:', err.message);
  }
}

// ═══════════════════════════════════════════════════════════════
// Health & Status
// ═══════════════════════════════════════════════════════════════

async function waitForHealthy() {
  console.log(`  Waiting for control plane at ${BASE_URL}...`);
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    try {
      const { status } = await fetchJson(`${BASE_URL}/v1/health`);
      if (status === 200) {
        console.log('  Control plane is healthy.');
        return;
      }
    } catch { /* not ready yet */ }
    await sleep(2000);
  }
  throw new Error('Control plane did not become healthy within 60s');
}

async function getAgents() {
  const { data } = await fetchJson(`${BASE_URL}/v1/agents`);
  return Array.isArray(data) ? data : [];
}

async function printStatus() {
  try {
    await waitForHealthy();
  } catch {
    console.log('Control plane is not running.');
    return;
  }

  const agents = await getAgents();
  if (agents.length === 0) {
    console.log('No agents found. Holding not initialized.');
    return;
  }

  console.log(`\nAgents (${agents.length}):`);
  for (const a of agents) {
    console.log(`  ${a.role.padEnd(8)} ${a.name.padEnd(25)} ${a.status.padEnd(10)} ${a.effective_model || ''}`);
  }
}

// ═══════════════════════════════════════════════════════════════
// Initialization
// ═══════════════════════════════════════════════════════════════

async function initHolding() {
  console.log('  Initializing new holding...');
  const { status, data } = await fetchJson(`${BASE_URL}/v1/install/init`, {
    method: 'POST',
    body: JSON.stringify({
      holding_name: 'PromptFoo Test Holding',
      main_agent_name: 'KonnerBot',
    }),
  });

  if (data.status === 'already_initialized') {
    console.log('  Holding already initialized — skipping (use --teardown first for a clean slate).');
    return false;
  }

  console.log('  Holding initialized.');
  return true;
}

async function waitForMainAgent() {
  console.log(`  Waiting for MAIN agent to boot (up to ${MAIN_BOOT_TIMEOUT}s)...`);
  const deadline = Date.now() + MAIN_BOOT_TIMEOUT * 1000;

  while (Date.now() < deadline) {
    const agents = await getAgents();
    const main = agents.find(a => a.role === 'MAIN' && a.status === 'ACTIVE');
    if (main) {
      console.log(`  MAIN agent "${main.name}" is active.`);
      return main;
    }
    await sleep(POLL_INTERVAL * 1000);
  }

  throw new Error(`MAIN agent did not become active within ${MAIN_BOOT_TIMEOUT}s`);
}

async function waitForCeo() {
  console.log(`  Waiting for MAIN to hire a CEO (up to ${CEO_HIRE_TIMEOUT}s)...`);
  const deadline = Date.now() + CEO_HIRE_TIMEOUT * 1000;

  while (Date.now() < deadline) {
    const agents = await getAgents();
    const ceo = agents.find(a => a.role === 'CEO' && a.status === 'ACTIVE');
    if (ceo) {
      console.log(`  CEO "${ceo.name}" is active.`);
      return ceo;
    }
    await sleep(POLL_INTERVAL * 1000);
  }

  throw new Error(`No CEO hired within ${CEO_HIRE_TIMEOUT}s`);
}

async function waitForManager() {
  console.log(`  Waiting for CEO to hire a manager (up to ${MANAGER_HIRE_TIMEOUT}s)...`);
  const deadline = Date.now() + MANAGER_HIRE_TIMEOUT * 1000;

  while (Date.now() < deadline) {
    const agents = await getAgents();
    const mgr = agents.find(a => a.role === 'MANAGER' && a.status === 'ACTIVE');
    if (mgr) {
      console.log(`  Manager "${mgr.name}" is active.`);
      return mgr;
    }
    await sleep(POLL_INTERVAL * 1000);
  }

  console.warn('  Warning: No manager hired within timeout. Tests targeting MANAGERs may fail.');
  return null;
}

// ═══════════════════════════════════════════════════════════════
// Main
// ═══════════════════════════════════════════════════════════════

async function teardown() {
  console.log('\n=== MultiClaw Teardown ===\n');
  await stopAllOpenClawContainers();
  await wipeDatabase();
  console.log('\nTeardown complete.\n');
}

async function setup(quick = false) {
  console.log('\n=== MultiClaw PromptFoo Setup ===\n');

  // Step 1: Teardown existing state
  console.log('[1/5] Cleaning previous state...');
  await stopAllOpenClawContainers();
  await wipeDatabase();

  // Step 2: Wait for control plane
  console.log('[2/5] Checking control plane...');
  await waitForHealthy();

  // Step 3: Init holding
  console.log('[3/5] Creating fresh holding...');
  await initHolding();

  // Step 4: Wait for MAIN
  console.log('[4/5] Waiting for MAIN agent...');
  await waitForMainAgent();

  if (quick) {
    console.log('[5/5] Quick mode — skipping org tree wait.\n');
    console.log('Setup complete (quick mode). MAIN agent is ready.\n');
    return;
  }

  // Step 5: Wait for org tree to populate
  console.log('[5/5] Waiting for org tree...');
  await waitForCeo();
  await waitForManager();

  console.log('\nSetup complete. Ready for PromptFoo evaluation.\n');

  // Print final status
  await printStatus();
}

// CLI
const args = process.argv.slice(2);

if (args.includes('--teardown')) {
  teardown().catch(err => { console.error(err); process.exit(1); });
} else if (args.includes('--status')) {
  printStatus().catch(err => { console.error(err); process.exit(1); });
} else {
  const quick = args.includes('--quick');
  setup(quick).catch(err => { console.error(err); process.exit(1); });
}
