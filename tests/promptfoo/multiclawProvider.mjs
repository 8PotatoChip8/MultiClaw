/**
 * MultiClaw PromptFoo Provider
 *
 * Custom provider that connects to a running MultiClaw control plane,
 * sends messages to agents via the DM API, and returns their responses.
 *
 * Config options (passed via promptfooconfig.yaml provider config):
 *   baseUrl:    Control plane URL (default: http://localhost:8080)
 *   role:       Target agent role to test: MAIN, CEO, MANAGER, WORKER (default: CEO)
 *   agentName:  Specific agent name to target (optional — picks first matching role)
 *   timeout:    Max seconds to wait for agent response (default: 120)
 *   pollInterval: Seconds between polls for response (default: 3)
 *   mode:       'dm' (default), 'user', 'observe', or 'e2e'
 *   settleTime: Seconds of inactivity before E2E considers cascade complete (default: 45)
 *   maxWait:    Max seconds to wait for E2E cascade to settle (default: 300)
 */

const DEFAULT_BASE_URL = 'http://localhost:8080';
const DEFAULT_TIMEOUT = 120;
const DEFAULT_POLL_INTERVAL = 3;

// Cache operator thread IDs per agent to avoid creating duplicates
const operatorThreadCache = new Map();

async function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

async function fetchJson(url, options = {}) {
  const res = await fetch(url, {
    headers: { 'Content-Type': 'application/json', ...options.headers },
    ...options,
  });
  const text = await res.text();
  let json;
  try {
    json = JSON.parse(text);
  } catch {
    json = { raw: text };
  }
  if (!res.ok) {
    throw new Error(`HTTP ${res.status}: ${JSON.stringify(json)}`);
  }
  return json;
}

/**
 * Extract text from a message content field.
 * Handles both string content ("hello") and object content ({"text": "hello"}).
 */
function extractText(content) {
  if (typeof content === 'string') return content;
  if (content && typeof content === 'object' && content.text) return content.text;
  return JSON.stringify(content);
}

/**
 * Find the MAIN agent (used as the sender for DMs to other agents).
 */
async function findMainAgent(baseUrl) {
  const agents = await fetchJson(`${baseUrl}/v1/agents`);
  const main = agents.find(a => a.role === 'MAIN' && a.status === 'ACTIVE');
  if (!main) throw new Error('No active MAIN agent found. Is the holding initialized?');
  return main;
}

/**
 * Find target agent by role (and optionally name).
 * Waits up to timeoutSecs for the agent to appear (useful when setup is still
 * creating agents or a prior test triggered a hire).
 */
async function findTargetAgent(baseUrl, role, agentName, timeoutSecs = 0, pollInterval = 5) {
  const deadline = Date.now() + timeoutSecs * 1000;

  do {
    const agents = await fetchJson(`${baseUrl}/v1/agents`);
    let candidates = agents.filter(a => a.role === role && a.status === 'ACTIVE');
    if (agentName) {
      candidates = candidates.filter(a => a.name === agentName);
    }
    if (candidates.length > 0) return candidates[0];

    if (timeoutSecs <= 0) break;

    const available = agents.map(a => `${a.name}(${a.role}:${a.status})`).join(', ');
    console.log(`  Waiting for active ${role} agent... (available: ${available})`);
    await sleep(pollInterval * 1000);
  } while (Date.now() < deadline);

  const agents = await fetchJson(`${baseUrl}/v1/agents`);
  const available = agents.map(a => `${a.name}(${a.role}:${a.status})`).join(', ');
  throw new Error(`No active ${role} agent found${agentName ? ` named "${agentName}"` : ''} after ${timeoutSecs}s. Available: ${available}`);
}

/**
 * Get all messages in a thread, sorted by creation time.
 */
async function getThreadMessages(baseUrl, threadId) {
  return fetchJson(`${baseUrl}/v1/threads/${threadId}/messages`);
}

/**
 * Find or get the DM thread between two agents.
 */
async function findDmThread(baseUrl, agent1Id, agent2Id) {
  const threads = await fetchJson(`${baseUrl}/v1/agents/${agent1Id}/threads`);
  for (const t of threads) {
    if (t.type !== 'DM') continue;
    const participants = await fetchJson(`${baseUrl}/v1/threads/${t.id}/participants`);
    const memberIds = participants.map(p => p.agent_id || p.member_id).filter(Boolean);
    if (memberIds.includes(agent1Id) && memberIds.includes(agent2Id)) {
      return t;
    }
  }
  return null;
}

/**
 * Send a DM from sender to target and wait for the target's response.
 * Returns the target agent's response text.
 */
async function sendDmAndWaitForResponse(baseUrl, senderId, targetId, message, timeoutSecs, pollInterval) {
  // Snapshot current message count in DM thread (if exists)
  let existingThread = await findDmThread(baseUrl, senderId, targetId);
  let messagesBefore = 0;
  if (existingThread) {
    const msgs = await getThreadMessages(baseUrl, existingThread.id);
    messagesBefore = msgs.length;
  }

  // Send the DM (retry on 409 — agent may be in an active DM conversation)
  const dmDeadline = Date.now() + timeoutSecs * 1000;
  while (true) {
    try {
      await fetchJson(`${baseUrl}/v1/agents/${senderId}/dm`, {
        method: 'POST',
        body: JSON.stringify({ target: targetId, message }),
      });
      break;
    } catch (err) {
      if (err.message.includes('HTTP 409') && Date.now() < dmDeadline) {
        console.log(`  DM blocked (agent in conversation) — retrying in ${pollInterval}s...`);
        await sleep(pollInterval * 1000);
        continue;
      }
      throw err;
    }
  }

  // Poll for the target's response
  const deadline = Date.now() + timeoutSecs * 1000;

  while (Date.now() < deadline) {
    await sleep(pollInterval * 1000);

    // Re-find thread (may have been created by the DM)
    if (!existingThread) {
      existingThread = await findDmThread(baseUrl, senderId, targetId);
      if (!existingThread) continue;
    }

    const msgs = await getThreadMessages(baseUrl, existingThread.id);
    if (msgs.length <= messagesBefore) continue;

    // Find new messages from the target agent
    const newMessages = msgs.slice(messagesBefore);
    const targetResponses = newMessages.filter(m =>
      m.sender_id === targetId && m.sender_type === 'AGENT'
    );

    if (targetResponses.length > 0) {
      return targetResponses.map(m => extractText(m.content)).join('\n\n');
    }
  }

  throw new Error(`Timeout: no response from target agent within ${timeoutSecs}s`);
}

/**
 * Get the operator DM thread for an agent. Uses the same endpoint as setup.mjs
 * (GET /v1/agents/:id/thread) which returns the existing operator thread or
 * creates one. Cached per agent to avoid creating multiple threads.
 */
async function getOperatorThread(baseUrl, agentId) {
  if (operatorThreadCache.has(agentId)) {
    return operatorThreadCache.get(agentId);
  }
  const data = await fetchJson(`${baseUrl}/v1/agents/${agentId}/thread`);
  const threadId = data.thread_id;
  operatorThreadCache.set(agentId, threadId);
  return threadId;
}

/**
 * Send a message as the operator/user to an agent and wait for response.
 * Uses the canonical operator thread endpoint to avoid creating duplicate threads.
 */
async function sendUserMessageAndWait(baseUrl, agentId, message, timeoutSecs, pollInterval) {
  const threadId = await getOperatorThread(baseUrl, agentId);

  const msgsBefore = await getThreadMessages(baseUrl, threadId);
  const countBefore = msgsBefore.length;

  // Post user message
  await fetchJson(`${baseUrl}/v1/threads/${threadId}/messages`, {
    method: 'POST',
    body: JSON.stringify({
      sender_type: 'USER',
      content: { text: message },
    }),
  });

  // Poll for agent response
  const deadline = Date.now() + timeoutSecs * 1000;
  while (Date.now() < deadline) {
    await sleep(pollInterval * 1000);
    const msgs = await getThreadMessages(baseUrl, threadId);
    const newMsgs = msgs.slice(countBefore);
    const agentMsgs = newMsgs.filter(m => m.sender_type === 'AGENT' && m.sender_id === agentId);
    if (agentMsgs.length > 0) {
      return agentMsgs.map(m => extractText(m.content)).join('\n\n');
    }
  }

  throw new Error(`Timeout: no response from agent within ${timeoutSecs}s`);
}

/**
 * Observe agent behavior after a heartbeat (no message sent — just watch what it does).
 * Useful for testing if agents narrate, fabricate, or act correctly on their own.
 */
async function observeHeartbeatBehavior(baseUrl, agentId, timeoutSecs, pollInterval) {
  // Get current message count across all agent threads
  const threads = await fetchJson(`${baseUrl}/v1/agents/${agentId}/threads`);
  const snapshot = {};
  for (const t of threads) {
    const msgs = await getThreadMessages(baseUrl, t.id);
    snapshot[t.id] = msgs.length;
  }

  // Wait for the agent's next heartbeat cycle to produce messages
  const deadline = Date.now() + timeoutSecs * 1000;
  const allNewMessages = [];

  while (Date.now() < deadline) {
    await sleep(pollInterval * 1000);

    for (const t of threads) {
      const msgs = await getThreadMessages(baseUrl, t.id);
      const prev = snapshot[t.id] || 0;
      if (msgs.length > prev) {
        const newMsgs = msgs.slice(prev).filter(m => m.sender_id === agentId);
        allNewMessages.push(...newMsgs);
        snapshot[t.id] = msgs.length;
      }
    }

    if (allNewMessages.length > 0) {
      return allNewMessages.map(m => extractText(m.content)).join('\n\n');
    }
  }

  return '[NO_OUTPUT: agent produced no messages during observation window]';
}

/**
 * E2E mode: Send a user directive to MAIN, wait for the full cascade to settle
 * (MAIN → CEO → Managers → Workers), then collect ALL agent messages produced
 * after the directive was sent.
 *
 * Returns a JSON array of {agent, role, thread_id, message} objects so assertions
 * can check every individual message for behavioral compliance.
 */
async function sendDirectiveAndCollectAll(baseUrl, mainAgentId, message, settleTime, maxWait, pollInterval) {
  // Build agent lookup table
  const agents = await fetchJson(`${baseUrl}/v1/agents`);
  const agentMap = new Map();
  for (const a of agents) {
    agentMap.set(a.id, { name: a.name, role: a.role });
  }

  // Get total message count across ALL threads before sending
  const allThreadsBefore = await fetchJson(`${baseUrl}/v1/threads?agent_only=true`);
  const userThreadsBefore = await fetchJson(`${baseUrl}/v1/threads`);
  const threadsBefore = [...allThreadsBefore, ...userThreadsBefore];
  // Deduplicate by thread ID
  const seenIds = new Set();
  const uniqueThreadsBefore = threadsBefore.filter(t => {
    if (seenIds.has(t.id)) return false;
    seenIds.add(t.id);
    return true;
  });

  let totalMsgsBefore = 0;
  const threadMsgCounts = {};
  for (const t of uniqueThreadsBefore) {
    const msgs = await getThreadMessages(baseUrl, t.id);
    threadMsgCounts[t.id] = msgs.length;
    totalMsgsBefore += msgs.length;
  }

  const startTime = new Date().toISOString();

  // Send the user directive to MAIN
  const threadId = await getOperatorThread(baseUrl, mainAgentId);
  await fetchJson(`${baseUrl}/v1/threads/${threadId}/messages`, {
    method: 'POST',
    body: JSON.stringify({
      sender_type: 'USER',
      content: { text: message },
    }),
  });

  console.log(`  [E2E] Directive sent to MAIN. Waiting for cascade to settle...`);

  // Wait for cascade to settle: no new messages for settleTime seconds
  const deadline = Date.now() + maxWait * 1000;
  let lastNewMsgTime = Date.now();
  let lastTotalMsgs = totalMsgsBefore;

  while (Date.now() < deadline) {
    await sleep(pollInterval * 1000);

    // Count total messages across all threads (including newly created ones)
    const agentThreads = await fetchJson(`${baseUrl}/v1/threads?agent_only=true`);
    const userThreads = await fetchJson(`${baseUrl}/v1/threads`);
    const allThreads = [...agentThreads, ...userThreads];
    const seen = new Set();
    const unique = allThreads.filter(t => {
      if (seen.has(t.id)) return false;
      seen.add(t.id);
      return true;
    });

    let currentTotal = 0;
    for (const t of unique) {
      const msgs = await getThreadMessages(baseUrl, t.id);
      currentTotal += msgs.length;
    }

    if (currentTotal > lastTotalMsgs) {
      const delta = currentTotal - lastTotalMsgs;
      console.log(`  [E2E] +${delta} new messages (total: ${currentTotal}). Resetting settle timer...`);
      lastTotalMsgs = currentTotal;
      lastNewMsgTime = Date.now();
    }

    const settledFor = (Date.now() - lastNewMsgTime) / 1000;
    if (settledFor >= settleTime && currentTotal > totalMsgsBefore) {
      console.log(`  [E2E] Cascade settled (no new messages for ${Math.round(settledFor)}s).`);
      break;
    }
  }

  // Refresh agent map (new agents may have been created during cascade)
  const agentsAfter = await fetchJson(`${baseUrl}/v1/agents`);
  for (const a of agentsAfter) {
    agentMap.set(a.id, { name: a.name, role: a.role });
  }

  // Collect all messages created after startTime from all threads
  const agentThreadsFinal = await fetchJson(`${baseUrl}/v1/threads?agent_only=true`);
  const userThreadsFinal = await fetchJson(`${baseUrl}/v1/threads`);
  const allThreadsFinal = [...agentThreadsFinal, ...userThreadsFinal];
  const seenFinal = new Set();
  const uniqueFinal = allThreadsFinal.filter(t => {
    if (seenFinal.has(t.id)) return false;
    seenFinal.add(t.id);
    return true;
  });

  const collected = [];
  for (const t of uniqueFinal) {
    const msgs = await getThreadMessages(baseUrl, t.id);
    for (const m of msgs) {
      // Only include messages created after the directive was sent
      if (m.created_at < startTime) continue;
      // Only include agent messages (not USER or SYSTEM)
      if (m.sender_type !== 'AGENT') continue;

      const agentInfo = agentMap.get(m.sender_id) || { name: 'Unknown', role: 'UNKNOWN' };
      const text = extractText(m.content);
      if (!text || text.trim().length === 0) continue;

      collected.push({
        agent: agentInfo.name,
        role: agentInfo.role,
        thread_id: t.id,
        message: text,
      });
    }
  }

  // Deduplicate by message content + agent (same message can appear in thread overlap)
  const dedupKey = (m) => `${m.agent}:${m.message}`;
  const seen2 = new Set();
  const deduped = collected.filter(m => {
    const k = dedupKey(m);
    if (seen2.has(k)) return false;
    seen2.add(k);
    return true;
  });

  console.log(`  [E2E] Collected ${deduped.length} agent messages from ${uniqueFinal.length} threads.`);

  return JSON.stringify(deduped);
}


// ═══════════════════════════════════════════════════════════════
// PromptFoo Provider Class
// ═══════════════════════════════════════════════════════════════

export default class MultiClawProvider {
  constructor(options) {
    const config = options.config || {};
    this.baseUrl = config.baseUrl || DEFAULT_BASE_URL;
    this.role = config.role || 'CEO';
    this.agentName = config.agentName || null;
    this.timeout = config.timeout || DEFAULT_TIMEOUT;
    this.pollInterval = config.pollInterval || DEFAULT_POLL_INTERVAL;
    this.mode = config.mode || 'dm'; // 'dm', 'user', 'observe', or 'e2e'
    this.settleTime = config.settleTime || 45;  // seconds of inactivity = cascade done
    this.maxWait = config.maxWait || 300;        // max seconds to wait for cascade

    this.id = () => `multiclaw:${this.mode === 'e2e' ? 'e2e' : this.role}${this.agentName ? `:${this.agentName}` : ''}`;
  }

  async callApi(prompt, context) {
    try {
      if (this.mode === 'e2e') {
        const mainAgent = await findMainAgent(this.baseUrl);
        const output = await sendDirectiveAndCollectAll(
          this.baseUrl, mainAgent.id, prompt,
          this.settleTime, this.maxWait, this.pollInterval
        );
        return { output };
      }

      if (this.mode === 'observe') {
        const target = await findTargetAgent(this.baseUrl, this.role, this.agentName, this.timeout, this.pollInterval);
        const output = await observeHeartbeatBehavior(
          this.baseUrl, target.id, this.timeout, this.pollInterval
        );
        return { output };
      }

      if (this.mode === 'user') {
        const target = await findTargetAgent(this.baseUrl, this.role, this.agentName, this.timeout, this.pollInterval);
        const output = await sendUserMessageAndWait(
          this.baseUrl, target.id, prompt, this.timeout, this.pollInterval
        );
        return { output };
      }

      // Default: DM mode — send from MAIN (or parent) to target agent
      const mainAgent = await findMainAgent(this.baseUrl);
      // Wait for target agent to appear (setup may still be hiring)
      const target = await findTargetAgent(this.baseUrl, this.role, this.agentName, this.timeout, this.pollInterval);

      // Determine who should send the DM based on hierarchy
      let senderId;
      if (this.role === 'MAIN') {
        const output = await sendUserMessageAndWait(
          this.baseUrl, target.id, prompt, this.timeout, this.pollInterval
        );
        return { output };
      } else if (this.role === 'CEO') {
        senderId = mainAgent.id;
      } else {
        const parentId = target.parent_agent_id;
        if (parentId) {
          senderId = parentId;
        } else {
          senderId = mainAgent.id;
        }
      }

      const output = await sendDmAndWaitForResponse(
        this.baseUrl, senderId, target.id, prompt, this.timeout, this.pollInterval
      );
      return { output };

    } catch (err) {
      return { error: err.message };
    }
  }
}
