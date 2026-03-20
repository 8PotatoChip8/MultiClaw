#!/usr/bin/env node

/**
 * MultiClaw PromptFoo Test Setup
 *
 * Prepares a fresh holding for PromptFoo evaluation:
 *   1. Calls POST /v1/system/reset to wipe everything and reinitialize
 *   2. Waits for the MAIN agent to boot and become responsive
 *   3. Sends operator messages to MAIN to create a company and hire a CEO
 *   4. Waits for CEO to become active, optionally waits for managers
 *
 * Usage:
 *   node setup.mjs                    # Full setup: reset + wait for CEO
 *   node setup.mjs --quick            # Just reset, don't wait for org tree
 *   node setup.mjs --status           # Show current holding status
 *
 * Environment variables:
 *   MULTICLAW_URL    Control plane URL (default: http://localhost:8080)
 */

const BASE_URL = process.env.MULTICLAW_URL || 'http://localhost:8080';

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
// Reset via API
// ═══════════════════════════════════════════════════════════════

async function resetHolding() {
  console.log('  Resetting via POST /v1/system/reset...');
  const { status, data } = await fetchJson(`${BASE_URL}/v1/system/reset`, {
    method: 'POST',
    body: JSON.stringify({
      holding_name: 'PromptFoo Test Holding',
      main_agent_name: 'KonnerBot',
    }),
  });

  if (status !== 200 || data.status !== 'reset_complete') {
    throw new Error(`Reset failed: ${JSON.stringify(data)}`);
  }

  console.log(`  Reset complete. Holding "${data.holding_name}" created with agent "${data.main_agent_name}".`);
  return data;
}

// ═══════════════════════════════════════════════════════════════
// Wait for Agents
// ═══════════════════════════════════════════════════════════════

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
// Send Operator Messages to MAIN
// ═══════════════════════════════════════════════════════════════

/**
 * Get or create the operator's DM thread with an agent.
 */
async function getOperatorThread(agentId) {
  const { data } = await fetchJson(`${BASE_URL}/v1/agents/${agentId}/thread`);
  return data.thread_id;
}

/**
 * Send a message as the operator and wait for the agent to respond.
 */
async function sendOperatorMessage(threadId, message, timeoutSecs = 120) {
  const msgsBefore = await fetchJson(`${BASE_URL}/v1/threads/${threadId}/messages`);
  const countBefore = Array.isArray(msgsBefore.data) ? msgsBefore.data.length : 0;

  await fetchJson(`${BASE_URL}/v1/threads/${threadId}/messages`, {
    method: 'POST',
    body: JSON.stringify({ sender_type: 'USER', content: { text: message } }),
  });

  console.log(`    Sent: "${message.substring(0, 80)}${message.length > 80 ? '...' : ''}"`);

  const deadline = Date.now() + timeoutSecs * 1000;
  while (Date.now() < deadline) {
    await sleep(POLL_INTERVAL * 1000);
    const msgsNow = await fetchJson(`${BASE_URL}/v1/threads/${threadId}/messages`);
    const allMsgs = Array.isArray(msgsNow.data) ? msgsNow.data : [];
    const newMsgs = allMsgs.slice(countBefore);
    const agentMsgs = newMsgs.filter(m => m.sender_type === 'AGENT');
    if (agentMsgs.length > 0) {
      const reply = agentMsgs.map(m =>
        typeof m.content === 'string' ? m.content : JSON.stringify(m.content)
      ).join('\n');
      console.log(`    Reply: "${reply.substring(0, 120)}${reply.length > 120 ? '...' : ''}"`);
      return reply;
    }
  }

  console.warn('    Warning: No response within timeout, continuing...');
  return null;
}

/**
 * Send operator messages to MAIN to kickstart company creation and CEO hiring.
 */
async function kickstartHolding(mainAgent) {
  console.log('  Getting operator DM thread with MAIN...');
  const threadId = await getOperatorThread(mainAgent.id);

  console.log('  Sending greeting...');
  await sendOperatorMessage(threadId, 'Hello');

  // Give MAIN a moment to settle after greeting
  await sleep(3000);

  console.log('  Requesting company creation...');
  await sendOperatorMessage(threadId,
    'Create a company called "PromptFoo Test Corp". ' +
    'It should be an internal software development company. ' +
    'Its purpose is to develop and maintain internal tools, APIs, and dashboards ' +
    'for other companies in the holding. Focus on quality, performance, and security. ' +
    'Hire a CEO and get it operational.',
    180
  );
}

// ═══════════════════════════════════════════════════════════════
// Main
// ═══════════════════════════════════════════════════════════════

async function setup(quick = false) {
  console.log('\n=== MultiClaw PromptFoo Setup ===\n');

  // Step 1: Check control plane is up
  console.log('[1/5] Checking control plane...');
  await waitForHealthy();

  // Step 2: Reset everything via API
  console.log('[2/5] Resetting holding (wipe + reinitialize)...');
  await resetHolding();

  // Step 3: Wait for MAIN
  console.log('[3/5] Waiting for MAIN agent...');
  const mainAgent = await waitForMainAgent();

  if (quick) {
    console.log('[4/5] Quick mode — skipping company creation.\n');
    console.log('[5/5] Skipped.\n');
    console.log('Setup complete (quick mode). MAIN agent is ready.\n');
    return;
  }

  // Step 4: Send messages to MAIN to create a company
  console.log('[4/5] Sending directives to MAIN agent...');
  await kickstartHolding(mainAgent);

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

if (args.includes('--status')) {
  printStatus().catch(err => { console.error(err); process.exit(1); });
} else {
  const quick = args.includes('--quick');
  setup(quick).catch(err => { console.error(err); process.exit(1); });
}
