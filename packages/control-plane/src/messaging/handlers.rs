//! Queue handler functions — one per message `kind`.
//!
//! Each handler receives the AppState and a JSON payload, executes the
//! agent-messaging logic that previously lived inside `tokio::spawn` blocks
//! in `routes.rs` and `main.rs`, and returns `Ok(())` on success or
//! `Err(String)` to signal the queue worker to retry or fail the item.
//!
//! IMPORTANT: The queue worker already holds the per-agent turn lock before
//! calling these handlers. Do NOT call `acquire_agent_turn()` inside handlers.

use crate::api::routes::{strip_agent_tags, scrub_secrets, word_overlap_ratio};
use crate::api::ws::AppState;
use crate::db::models::Message;
use serde_json::json;
use uuid::Uuid;

/// Helper: extract a UUID from a JSON value.
fn uuid_from_json(v: &serde_json::Value, key: &str) -> Result<Uuid, String> {
    v.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing '{}' in payload", key))
        .and_then(|s| Uuid::parse_str(s).map_err(|e| format!("invalid UUID for '{}': {}", key, e)))
}

fn str_from_json<'a>(v: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    v.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing '{}' in payload", key))
}

fn i32_from_json(v: &serde_json::Value, key: &str) -> Result<i32, String> {
    v.get(key)
        .and_then(|v| v.as_i64())
        .map(|n| n as i32)
        .ok_or_else(|| format!("missing '{}' in payload", key))
}

// ═══════════════════════════════════════════════════════════════
// thread_reply — User/agent message in a thread triggers agent response
// ═══════════════════════════════════════════════════════════════

/// Build context-aware instructions for an agent responding in a thread.
/// Includes recent conversation history, participant names, sender identity, and agent role.
async fn build_thread_context(
    state: &AppState,
    thread_id: Uuid,
    responding_agent_id: Uuid,
    sender_id: Uuid,
    is_agent_sender: bool,
) -> String {
    let thread_type: String = sqlx::query_scalar("SELECT type FROM threads WHERE id = $1")
        .bind(thread_id).fetch_optional(&state.db).await.ok().flatten()
        .unwrap_or_else(|| "DM".to_string());

    let thread_title: Option<String> = sqlx::query_scalar("SELECT title FROM threads WHERE id = $1")
        .bind(thread_id).fetch_optional(&state.db).await.ok().flatten();

    let participant_names: Vec<String> = sqlx::query_scalar(
        "SELECT a.name FROM agents a JOIN thread_members tm ON a.id = tm.member_id \
         WHERE tm.thread_id = $1 AND tm.member_type = 'AGENT'"
    ).bind(thread_id).fetch_all(&state.db).await.unwrap_or_default();

    let sender_label = if is_agent_sender {
        let name: Option<String> = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
            .bind(sender_id).fetch_optional(&state.db).await.ok().flatten();
        name.unwrap_or_else(|| "an agent".to_string())
    } else {
        "the human operator".to_string()
    };

    let base_context = if thread_type == "GROUP" {
        let title = thread_title.as_deref().unwrap_or("Group Chat");
        let members = participant_names.join(", ");
        format!(
            "You are responding in the group thread '{}' (participants: {}). The message is from {}. \
             Send ONLY your direct response. \
             Do NOT narrate your actions (e.g., 'Let me check...', 'I'll look into...', 'Sending a DM now...'). \
             Do NOT include internal thoughts, planning steps, or tool-use commentary. \
             The other participants see everything you write.",
            title, members, sender_label
        )
    } else {
        format!(
            "You are in a direct message with {}. \
             Send ONLY your direct response. \
             Do NOT narrate your actions (e.g., 'Let me check...', 'I'll look into...', 'Sending a DM now...'). \
             Do NOT include internal thoughts, planning steps, or tool-use commentary. \
             {} sees everything you write.",
            sender_label, sender_label
        )
    };

    // Fetch recent thread history for conversation context
    let recent_msgs: Vec<(String, Uuid, serde_json::Value)> = sqlx::query_as(
        "SELECT sender_type, sender_id, content FROM messages \
         WHERE thread_id = $1 ORDER BY created_at DESC LIMIT 20"
    ).bind(thread_id).fetch_all(&state.db).await.unwrap_or_default();

    let with_history = if recent_msgs.len() > 1 {
        let mut name_cache: std::collections::HashMap<Uuid, String> = std::collections::HashMap::new();
        for (stype, sid, _) in &recent_msgs {
            if stype == "AGENT" && !name_cache.contains_key(sid) {
                let aname: Option<String> = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
                    .bind(sid).fetch_optional(&state.db).await.ok().flatten();
                if let Some(n) = aname { name_cache.insert(*sid, n); }
            }
        }

        let mut transcript = String::from("Recent conversation history (most recent last):\n---\n");
        for (stype, sid, content) in recent_msgs.iter().rev().take(recent_msgs.len() - 1) {
            if stype == "SYSTEM" { continue; }
            let name = if stype == "USER" {
                "Operator".to_string()
            } else {
                name_cache.get(sid).cloned().unwrap_or_else(|| "Agent".to_string())
            };
            let text = content.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let truncated = if text.len() > 200 {
                let end = text.char_indices().take_while(|(i, _)| *i < 200).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(200);
                format!("{}...", &text[..end])
            } else { text.to_string() };
            transcript.push_str(&format!("{}: {}\n", name, truncated));
        }
        transcript.push_str("---\n");
        format!("{}\n\n{}", transcript, base_context)
    } else {
        base_context
    };

    // Prepend per-agent identity
    let agent_role: Option<String> = sqlx::query_scalar("SELECT role FROM agents WHERE id = $1")
        .bind(responding_agent_id).fetch_optional(&state.db).await.ok().flatten();
    let agent_name_ctx: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
        .bind(responding_agent_id).fetch_optional(&state.db).await.ok().flatten()
        .unwrap_or_else(|| "Agent".to_string());
    let role_str = match agent_role.as_deref() {
        Some("CEO") => "CEO", Some("MANAGER") => "Manager",
        Some("WORKER") => "Worker", _ => "member",
    };

    format!("You are {} ({}). {}", agent_name_ctx, role_str, with_history)
}

/// Handles a message in a thread that needs a single agent's response.
/// Payload: { thread_id, message_text, sender_id, sender_type, reply_depth, responding_agent_id }
///
/// The enqueuer creates one queue item per responding agent with the correct agent_id.
pub async fn handle_thread_reply(state: &AppState, payload: &serde_json::Value) -> Result<(), String> {
    let thread_id = uuid_from_json(payload, "thread_id")?;
    let message_text = str_from_json(payload, "message_text")?;
    let sender_id = uuid_from_json(payload, "sender_id")?;
    let sender_type = str_from_json(payload, "sender_type")?;
    let reply_depth = i32_from_json(payload, "reply_depth")?;
    let responding_agent_id = uuid_from_json(payload, "responding_agent_id")?;
    let is_agent_sender = sender_type == "AGENT";
    let next_depth = reply_depth + 1;

    // Build full thread context with history and agent identity
    let agent_context = build_thread_context(
        state, thread_id, responding_agent_id, sender_id, is_agent_sender,
    ).await;

    // Track that this agent is responding to a user in a DM thread
    let thread_type: String = sqlx::query_scalar("SELECT type FROM threads WHERE id = $1")
        .bind(thread_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "DM".to_string());

    if !is_agent_sender && thread_type == "DM" {
        state.responding_to_user.write().await.insert(responding_agent_id, thread_id);
    }

    state.mark_agent_working(responding_agent_id, "Responding in thread").await;
    let result: Result<String, String> = match state.openclaw.send_message(responding_agent_id, message_text, Some(&agent_context)).await {
        Ok(response) => {
            tracing::info!("OpenClaw responded for agent {}", responding_agent_id);
            Ok(response)
        }
        Err(e) => {
            tracing::warn!("OpenClaw unavailable for agent {}: {}", responding_agent_id, e);
            let agent_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
                .bind(responding_agent_id)
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "Agent".to_string());
            Ok(format!(
                "⚠️ {} is currently unavailable — their OpenClaw runtime is not running. \
                 Please wait for their instance to come online before sending messages.",
                agent_name
            ))
        }
    };
    state.mark_agent_done(responding_agent_id).await;

    match result {
        Ok(response) => {
            let (cleaned, _) = strip_agent_tags(&response);
            let scrubbed = if let Some(ref crypto) = state.crypto {
                scrub_secrets(&state.db, crypto, responding_agent_id, &cleaned).await
            } else {
                cleaned
            };
            if scrubbed.trim().is_empty() {
                tracing::warn!(
                    "Agent {} response on thread {} stripped to empty (original {} chars)",
                    responding_agent_id, thread_id, response.len()
                );
            } else {
                let resp_id = Uuid::new_v4();
                let content = json!({"text": scrubbed});
                if let Ok(agent_msg) = sqlx::query_as::<_, Message>(
                    "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,'AGENT',$3,$4,$5) \
                     RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
                )
                .bind(resp_id)
                .bind(thread_id)
                .bind(responding_agent_id)
                .bind(&content)
                .bind(next_depth)
                .fetch_one(&state.db)
                .await
                {
                    let _ = state.tx.send(json!({"type": "new_message", "message": agent_msg}).to_string());
                    tracing::info!("Agent {} responded on thread {}", responding_agent_id, thread_id);
                }
            }
        }
        Err(e) => {
            tracing::error!("Agent error: {}", e);
            let resp_id = Uuid::new_v4();
            let content = json!({"text": format!("Sorry, I encountered an error: {}", e)});
            let _ = sqlx::query(
                "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,'AGENT',$3,$4,$5)"
            )
            .bind(resp_id)
            .bind(thread_id)
            .bind(responding_agent_id)
            .bind(&content)
            .bind(next_depth)
            .execute(&state.db)
            .await;
        }
    }

    // Clear the responding-to-user tracking
    if !is_agent_sender && thread_type == "DM" {
        state.responding_to_user.write().await.remove(&responding_agent_id);
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// dm_initiate — First message of an agent-to-agent DM (target responds)
// ═══════════════════════════════════════════════════════════════

/// Handles the first turn of an agent-to-agent DM conversation.
/// The initial message has already been stored to DB by the route handler.
/// This handler sends it to the target agent and, on success, enqueues a dm_continue.
///
/// Payload: { thread_id, sender_id, target_id, message_text, pair_key_a, pair_key_b }
pub async fn handle_dm_initiate(state: &AppState, payload: &serde_json::Value) -> Result<(), String> {
    let thread_id = uuid_from_json(payload, "thread_id")?;
    let sender_id = uuid_from_json(payload, "sender_id")?;
    let target_id = uuid_from_json(payload, "target_id")?;
    let message_text = str_from_json(payload, "message_text")?;

    // The responder for the first turn is the target
    let responder_id = target_id;
    let other_id = sender_id;

    // Run one DM turn
    let turn_result = run_dm_turn(state, thread_id, responder_id, other_id, message_text, 0).await;

    match turn_result {
        DmTurnResult::Continue { response_text, next_depth } => {
            // Enqueue the next turn with swapped roles
            let _ = state.enqueue_message(
                other_id, // now the other agent responds
                3, // agent DM priority
                "dm_continue",
                json!({
                    "thread_id": thread_id.to_string(),
                    "responder_id": other_id.to_string(),
                    "other_id": responder_id.to_string(),
                    "message_text": response_text,
                    "depth": next_depth,
                    "sender_id": sender_id.to_string(),
                    "target_id": target_id.to_string(),
                    "pair_key_a": payload.get("pair_key_a").and_then(|v| v.as_str()).unwrap_or(""),
                    "pair_key_b": payload.get("pair_key_b").and_then(|v| v.as_str()).unwrap_or(""),
                }),
            ).await.map_err(|e| format!("Failed to enqueue dm_continue: {}", e))?;
            Ok(())
        }
        DmTurnResult::End => {
            // Conversation ended on the first reply — run cleanup
            dm_cleanup(state, sender_id, target_id, payload).await;
            Ok(())
        }
        DmTurnResult::Error(e) => {
            dm_cleanup(state, sender_id, target_id, payload).await;
            Err(e)
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// dm_continue — Subsequent turns of an agent-to-agent DM
// ═══════════════════════════════════════════════════════════════

/// Handles one turn of an ongoing agent-to-agent DM conversation.
/// On success, enqueues the next turn (with swapped roles) unless the
/// conversation has ended.
///
/// Payload: { thread_id, responder_id, other_id, message_text, depth, sender_id, target_id, pair_key_a, pair_key_b }
pub async fn handle_dm_continue(state: &AppState, payload: &serde_json::Value) -> Result<(), String> {
    let thread_id = uuid_from_json(payload, "thread_id")?;
    let responder_id = uuid_from_json(payload, "responder_id")?;
    let other_id = uuid_from_json(payload, "other_id")?;
    let message_text = str_from_json(payload, "message_text")?;
    let depth = i32_from_json(payload, "depth")?;
    let sender_id = uuid_from_json(payload, "sender_id")?;
    let target_id = uuid_from_json(payload, "target_id")?;

    let turn_result = run_dm_turn(state, thread_id, responder_id, other_id, message_text, depth).await;

    match turn_result {
        DmTurnResult::Continue { response_text, next_depth } => {
            // Safety ceiling
            if next_depth >= 20 {
                tracing::warn!("DM conversation on thread {} hit safety limit", thread_id);
                dm_cleanup(state, sender_id, target_id, payload).await;
                return Ok(());
            }

            // Enqueue next turn with swapped roles
            let _ = state.enqueue_message(
                other_id, // now the other agent responds
                3,
                "dm_continue",
                json!({
                    "thread_id": thread_id.to_string(),
                    "responder_id": other_id.to_string(),
                    "other_id": responder_id.to_string(),
                    "message_text": response_text,
                    "depth": next_depth,
                    "sender_id": sender_id.to_string(),
                    "target_id": target_id.to_string(),
                    "pair_key_a": payload.get("pair_key_a").and_then(|v| v.as_str()).unwrap_or(""),
                    "pair_key_b": payload.get("pair_key_b").and_then(|v| v.as_str()).unwrap_or(""),
                }),
            ).await.map_err(|e| format!("Failed to enqueue dm_continue: {}", e))?;
            Ok(())
        }
        DmTurnResult::End => {
            dm_cleanup(state, sender_id, target_id, payload).await;
            Ok(())
        }
        DmTurnResult::Error(e) => {
            dm_cleanup(state, sender_id, target_id, payload).await;
            Err(e)
        }
    }
}

/// Result of a single DM turn.
enum DmTurnResult {
    /// Conversation continues — contains the response text and next depth counter.
    Continue { response_text: String, next_depth: i32 },
    /// Conversation ended naturally (END_CONVERSATION, empty, redundant, quarantined).
    End,
    /// Error occurred.
    Error(String),
}

/// Execute one turn of a DM conversation: send message to responder, store response, check end conditions.
async fn run_dm_turn(
    state: &AppState,
    thread_id: Uuid,
    responder_id: Uuid,
    other_id: Uuid,
    current_text: &str,
    current_depth: i32,
) -> DmTurnResult {
    // Check if responder is quarantined
    let responder_status: Option<String> = sqlx::query_scalar("SELECT status FROM agents WHERE id = $1")
        .bind(responder_id).fetch_optional(&state.db).await.ok().flatten();
    if responder_status.as_deref() == Some("QUARANTINED") {
        tracing::info!("DM on thread {} stopped: agent {} is quarantined", thread_id, responder_id);
        return DmTurnResult::End;
    }

    // Build DM context instructions
    let partner_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
        .bind(other_id).fetch_optional(&state.db).await.ok().flatten()
        .unwrap_or_else(|| "a colleague".into());
    let responder_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
        .bind(responder_id).fetch_optional(&state.db).await.ok().flatten()
        .unwrap_or_else(|| "Agent".into());
    let responder_role: Option<String> = sqlx::query_scalar("SELECT role FROM agents WHERE id = $1")
        .bind(responder_id).fetch_optional(&state.db).await.ok().flatten();
    let partner_role: Option<String> = sqlx::query_scalar("SELECT role FROM agents WHERE id = $1")
        .bind(other_id).fetch_optional(&state.db).await.ok().flatten();
    let responder_parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_agent_id FROM agents WHERE id = $1")
        .bind(responder_id).fetch_optional(&state.db).await.ok().flatten();
    let partner_parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_agent_id FROM agents WHERE id = $1")
        .bind(other_id).fetch_optional(&state.db).await.ok().flatten();
    let responder_company: Option<String> = sqlx::query_scalar(
        "SELECT c.name FROM companies c JOIN agents a ON a.company_id = c.id WHERE a.id = $1"
    ).bind(responder_id).fetch_optional(&state.db).await.ok().flatten();

    let role_label = |r: &Option<String>| match r.as_deref() {
        Some("CEO") => "CEO", Some("MANAGER") => "Manager",
        Some("WORKER") => "Worker", Some("MAIN") => "Head of Holdings", _ => "colleague",
    };
    let relationship = if responder_parent == Some(other_id) {
        "They are your superior — you report to them."
    } else if partner_parent == Some(responder_id) {
        "They report to you."
    } else if responder_parent == partner_parent && responder_parent.is_some() {
        "They are your peer — you share the same manager."
    } else { "" };

    let company_label = responder_company.as_deref().unwrap_or("the company");
    let dm_ctx = format!(
        "You are {} ({} at {}). You are in a DM with {} ({}). {} \
         Before responding, use memory_search to recall relevant context about this person and topic. \
         After the conversation, save important decisions, agreements, or new information to MEMORY.md. \
         Communicate naturally — ask questions, share information, and respond as needed. \
         Send ONLY your actual message to {}. \
         Do NOT repeat or rephrase information you already sent earlier in this conversation — they already received it. Only contribute NEW information, answers, or follow-ups. \
         NEVER narrate your actions or thinking (e.g., 'Let me check...', 'I'll review...', 'Sending now...'). \
         NEVER include planning steps, tool-use commentary, or internal reasoning — {} sees everything you write. \
         Do NOT include approval prompts, action requests, or instructions meant for the human operator — {} cannot act on those. Use the dm-user API to reach the operator separately. \
         When the conversation has reached a natural conclusion and you have nothing more to add, \
         end your final message with the exact tag [END_CONVERSATION] on its own line. \
         If the conversation has devolved into mutual acknowledgments or pleasantries with no new information \
         being exchanged (e.g., 'Understood', 'Got it', 'Sounds good', 'Will do'), that IS a natural conclusion — \
         use [END_CONVERSATION]. Do not keep exchanging acknowledgments back and forth. \
         Do NOT use this tag if {} asked you a question or if there are unresolved topics.",
        responder_name, role_label(&responder_role), company_label,
        partner_name, role_label(&partner_role), relationship,
        partner_name, partner_name, partner_name, partner_name
    );

    // Send message
    state.mark_agent_working(responder_id, "Chatting in DM").await;
    let result = state.openclaw.send_message(responder_id, current_text, Some(&dm_ctx)).await;
    state.mark_agent_done(responder_id).await;

    match result {
        Ok(response) => {
            let (clean_response, conversation_complete) = strip_agent_tags(&response);
            let scrubbed = if let Some(ref crypto) = state.crypto {
                scrub_secrets(&state.db, crypto, responder_id, &clean_response).await
            } else { clean_response.clone() };

            // Redundancy check
            if !scrubbed.trim().is_empty() {
                let prev_text: Option<String> = sqlx::query_scalar(
                    "SELECT content->>'text' FROM messages \
                     WHERE thread_id = $1 AND sender_id = $2 \
                     ORDER BY created_at DESC LIMIT 1"
                ).bind(thread_id).bind(responder_id)
                .fetch_optional(&state.db).await.ok().flatten();

                if let Some(prev) = prev_text {
                    let overlap = word_overlap_ratio(&prev, &scrubbed);
                    if overlap > 0.6 {
                        tracing::info!("DM thread {}: suppressed {}'s redundant re-statement (overlap {:.0}%)", thread_id, responder_id, overlap * 100.0);
                        return DmTurnResult::End;
                    }
                }
            }

            // Store message if non-empty
            if !scrubbed.trim().is_empty() {
                let resp_id = Uuid::new_v4();
                let resp_content = json!({"text": scrubbed});
                if let Ok(agent_msg) = sqlx::query_as::<_, Message>(
                    "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,'AGENT',$3,$4,$5) \
                     RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
                ).bind(resp_id).bind(thread_id).bind(responder_id).bind(&resp_content).bind(current_depth)
                .fetch_one(&state.db).await {
                    let _ = state.tx.send(json!({"type":"new_message","message": agent_msg}).to_string());
                }
            }

            // Check end conditions
            if clean_response.trim().is_empty() {
                tracing::info!("DM on thread {} ended: empty response after tag stripping (depth {})", thread_id, current_depth);
                return DmTurnResult::End;
            }

            if conversation_complete {
                tracing::info!("DM on thread {} ended naturally at depth {}", thread_id, current_depth + 1);
                return DmTurnResult::End;
            }

            DmTurnResult::Continue {
                response_text: clean_response,
                next_depth: current_depth + 1,
            }
        }
        Err(e) => {
            tracing::warn!("OpenClaw unavailable for agent {}: {}", responder_id, e);
            let agent_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
                .bind(responder_id).fetch_optional(&state.db).await.ok().flatten()
                .unwrap_or_else(|| "Agent".into());

            // Post a SYSTEM message
            let resp_id = Uuid::new_v4();
            let resp_content = json!({"text": format!("{} is currently unavailable. Your message was not delivered — please try again later.", agent_name)});
            let _ = sqlx::query(
                "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,'SYSTEM',$3,$4,$5)"
            ).bind(resp_id).bind(thread_id).bind(responder_id).bind(&resp_content).bind(current_depth)
            .execute(&state.db).await;
            let _ = state.tx.send(json!({"type":"new_message","thread_id": thread_id, "system": true, "text": format!("{} is currently unavailable.", agent_name)}).to_string());

            DmTurnResult::Error(format!("OpenClaw unavailable for {}: {}", agent_name, e))
        }
    }
}

/// Post-DM cleanup: mark agents idle, record cooldowns, release active pair, optionally enqueue action prompt.
async fn dm_cleanup(state: &AppState, sender_id: Uuid, target_id: Uuid, _payload: &serde_json::Value) {
    state.mark_agent_done(sender_id).await;
    state.mark_agent_done(target_id).await;

    // Record cooldown
    let pair_key = if sender_id < target_id { (sender_id, target_id) } else { (target_id, sender_id) };
    {
        let mut cooldowns = state.dm_cooldowns.write().await;
        cooldowns.insert(pair_key, tokio::time::Instant::now());
    }
    // Release active-conversation lock
    {
        let mut active = state.active_dm_pairs.write().await;
        active.remove(&pair_key);
    }

    // Post-conversation action prompt for CEO/MANAGER targets
    let target_role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM agents WHERE id = $1 AND status = 'ACTIVE'"
    ).bind(target_id).fetch_optional(&state.db).await.ok().flatten();

    if matches!(target_role.as_deref(), Some("CEO") | Some("MANAGER")) {
        let should_skip = {
            let cooldowns = state.action_prompt_cooldowns.read().await;
            cooldowns.get(&target_id)
                .map(|t| t.elapsed() < std::time::Duration::from_secs(120))
                .unwrap_or(false)
        };

        if !should_skip {
            // Record cooldown before enqueuing (so concurrent DMs see it immediately)
            state.action_prompt_cooldowns.write().await
                .insert(target_id, tokio::time::Instant::now());

            let sender_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
                .bind(sender_id).fetch_optional(&state.db).await.ok().flatten()
                .unwrap_or_else(|| "your colleague".into());

            let _ = state.enqueue_message(
                target_id,
                5, // routine priority
                "action_prompt",
                json!({
                    "agent_id": target_id.to_string(),
                    "sender_name": sender_name,
                }),
            ).await;
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// action_prompt — Post-DM nudge to act on what was discussed
// ═══════════════════════════════════════════════════════════════

pub async fn handle_action_prompt(state: &AppState, payload: &serde_json::Value) -> Result<(), String> {
    let agent_id = uuid_from_json(payload, "agent_id")?;
    let sender_name = str_from_json(payload, "sender_name")?;

    let action_prompt = format!(
        "SYSTEM: The conversation with {} has concluded. \
         Based on what was discussed, take any NEW actions you need to. \
         Do NOT repeat actions you have already taken — do not re-hire staff you already hired, \
         do not re-brief workers you already briefed, do not restart work already in progress. \
         Check your existing team and threads before taking action. \
         If everything discussed is already handled, respond with just: [NO_ACTION_NEEDED] \
         Do NOT repeat or summarize the conversation — just act on what is NEW. \
         After completing actions, save key outcomes and decisions to MEMORY.md (long-term) \
         or today's daily log in memory/ (working notes).",
        sender_name
    );

    let action_instructions = "You just finished receiving a briefing or directive. \
        Execute on it immediately using your available tools — but only NEW actions. \
        Do not repeat hiring, briefing, or tasks you have already completed. \
        IMPORTANT: When messaging other agents (workers, managers, etc.), use the `dm` endpoint \
        (POST /v1/agents/{YOUR_ID}/dm with {\"target\": \"AGENT_ID_OR_HANDLE\", \"message\": \"...\"}). \
        The `dm-user` endpoint is ONLY for contacting the human operator — never use it to message agents. \
        Be concise. Only respond with actions taken or [NO_ACTION_NEEDED].";

    state.mark_agent_working(agent_id, "Acting on briefing").await;
    match state.openclaw.send_message(agent_id, &action_prompt, Some(action_instructions)).await {
        Ok(response) => {
            let (cleaned, _) = strip_agent_tags(&response);
            let normalized = cleaned.replace('[', "").replace(']', "").replace('\n', " ").replace(' ', "");
            if normalized.trim().is_empty() || normalized.trim().eq_ignore_ascii_case("NOACTIONNEEDED") {
                tracing::debug!("Post-DM: {} has no immediate actions", agent_id);
            } else {
                tracing::info!("Post-DM: {} took action ({} chars)", agent_id, cleaned.len());
            }
        }
        Err(e) => {
            tracing::warn!("Post-DM action prompt failed for {}: {}", agent_id, e);
        }
    }
    state.mark_agent_done(agent_id).await;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// heartbeat — Periodic heartbeat to MAIN agent
// ═══════════════════════════════════════════════════════════════

pub async fn handle_heartbeat(state: &AppState, payload: &serde_json::Value) -> Result<(), String> {
    let agent_id = uuid_from_json(payload, "agent_id")?;
    let prompt = str_from_json(payload, "prompt")?;
    let instructions = payload.get("instructions").and_then(|v| v.as_str());

    state.mark_agent_working(agent_id, "Heartbeat").await;
    let result = state.openclaw.send_message(agent_id, prompt, instructions).await;
    state.mark_agent_done(agent_id).await;

    match result {
        Ok(response) => {
            let trimmed = response.trim();
            let (cleaned, _) = strip_agent_tags(trimmed);
            let has_heartbeat_tag = {
                let normalized: String = trimmed.chars()
                    .filter(|c| !c.is_whitespace() && *c != '[' && *c != ']')
                    .collect();
                normalized.contains("HEARTBEAT_OK") || normalized.contains("HEARTBEATOK")
            };

            if has_heartbeat_tag {
                tracing::debug!("[heartbeat] agent {} reports all clear", agent_id);
            } else if cleaned.is_empty() {
                tracing::debug!("[heartbeat] agent {} response was empty after cleaning", agent_id);
            } else {
                tracing::info!("[heartbeat] agent {} has a report ({}B)", agent_id, cleaned.len());
                // Store the cleaned response in the MAIN agent's human-operator DM thread
                let thread_id: Option<Uuid> = sqlx::query_scalar(
                    "SELECT tm.thread_id FROM thread_members tm \
                     JOIN threads t ON t.id = tm.thread_id \
                     JOIN thread_members tm2 ON t.id = tm2.thread_id \
                     WHERE tm.member_type = 'AGENT' AND tm.member_id = $1 \
                       AND tm2.member_type = 'USER' AND t.type = 'DM' LIMIT 1"
                ).bind(agent_id).fetch_optional(&state.db).await.ok().flatten();

                if let Some(tid) = thread_id {
                    let msg_id = Uuid::new_v4();
                    let content = json!({"text": cleaned});
                    if let Ok(agent_msg) = sqlx::query_as::<_, Message>(
                        "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) \
                         VALUES ($1,$2,'AGENT',$3,$4,0) \
                         RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
                    ).bind(msg_id).bind(tid).bind(agent_id).bind(&content)
                    .fetch_one(&state.db).await {
                        let _ = state.tx.send(json!({"type":"new_message","message": agent_msg}).to_string());
                    }
                }
            }
            Ok(())
        }
        Err(e) => Err(format!("Heartbeat failed for {}: {}", agent_id, e)),
    }
}

// ═══════════════════════════════════════════════════════════════
// Generic handlers — Simplified wrappers for less complex spawn sites
// ═══════════════════════════════════════════════════════════════

/// Generic send: just sends a message to an agent and optionally stores the response.
/// Payload: { agent_id, message, instructions?, task_label?, thread_id?, store_response? }
pub async fn handle_generic_send(state: &AppState, payload: &serde_json::Value) -> Result<(), String> {
    let agent_id = uuid_from_json(payload, "agent_id")?;
    let message = str_from_json(payload, "message")?;
    let instructions = payload.get("instructions").and_then(|v| v.as_str());
    let task_label = payload.get("task_label").and_then(|v| v.as_str()).unwrap_or("Processing message");

    state.mark_agent_working(agent_id, task_label).await;
    let result = state.openclaw.send_message(agent_id, message, instructions).await;
    state.mark_agent_done(agent_id).await;

    match result {
        Ok(response) => {
            let (cleaned, _) = strip_agent_tags(&response);
            let scrubbed = if let Some(ref crypto) = state.crypto {
                scrub_secrets(&state.db, crypto, agent_id, &cleaned).await
            } else { cleaned };

            // Optionally store the response in a thread
            if let Some(thread_id_str) = payload.get("thread_id").and_then(|v| v.as_str()) {
                if let Ok(thread_id) = Uuid::parse_str(thread_id_str) {
                    if !scrubbed.trim().is_empty() {
                        let resp_id = Uuid::new_v4();
                        let content = json!({"text": scrubbed});
                        if let Ok(agent_msg) = sqlx::query_as::<_, Message>(
                            "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,'AGENT',$3,$4,0) \
                             RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
                        ).bind(resp_id).bind(thread_id).bind(agent_id).bind(&content)
                        .fetch_one(&state.db).await {
                            let _ = state.tx.send(json!({"type":"new_message","message": agent_msg}).to_string());
                        }
                    }
                }
            }

            tracing::info!("[generic_send] agent {} responded ({} chars)", agent_id, scrubbed.len());
            Ok(())
        }
        Err(e) => Err(format!("send_message failed for {}: {}", agent_id, e)),
    }
}

/// Stub handlers for kinds that will be migrated incrementally.
/// These currently just call generic_send or do minimal work.

pub async fn handle_hire_notify(state: &AppState, payload: &serde_json::Value) -> Result<(), String> {
    handle_generic_send(state, payload).await
}

pub async fn handle_approval_escalate(state: &AppState, payload: &serde_json::Value) -> Result<(), String> {
    handle_generic_send(state, payload).await
}

pub async fn handle_file_notify(state: &AppState, payload: &serde_json::Value) -> Result<(), String> {
    handle_generic_send(state, payload).await
}

/// Handles a post-restart recovery prompt for an agent.
/// Payload: { agent_id, agent_name, role, restart_time }
pub async fn handle_recovery_prompt(state: &AppState, payload: &serde_json::Value) -> Result<(), String> {
    let agent_id = uuid_from_json(payload, "agent_id")?;
    let agent_name = str_from_json(payload, "agent_name")?;
    let role = str_from_json(payload, "role")?;
    let restart_time = str_from_json(payload, "restart_time")?;

    let prompt = match role {
        "MAIN" => format!(
            "SYSTEM RESTART NOTICE: The system was restarted at {}. \
             All agent containers have been recovered. Your in-memory context from before the restart is gone. \
             Review your current situation: \
             1. Check your org tree (companies and their CEOs) \
             2. Check your recent threads and messages for any in-progress work \
             3. Check your workspace memory for any saved state \
             4. Resume any interrupted work or verify everything is on track \
             If everything looks good and nothing needs immediate attention, respond with just: [RECOVERY_OK] \
             If there are issues or interrupted work to resume, briefly describe what you're doing about it.",
            restart_time
        ),
        "CEO" => format!(
            "SYSTEM RESTART NOTICE: The system was restarted at {}. \
             Your container has been recovered but your in-memory context is gone. \
             Review your situation: \
             1. Check your team (managers and workers under you) \
             2. Check your recent threads and DMs for any in-progress work \
             3. Check your workspace memory for saved state \
             4. Resume any interrupted work — do NOT re-hire people you already hired or re-brief people already briefed \
             If everything is on track, respond with: [RECOVERY_OK] \
             If you need to resume something, briefly act on it.",
            restart_time
        ),
        "MANAGER" => format!(
            "SYSTEM RESTART NOTICE: The system was restarted at {}. \
             Your container has been recovered but your in-memory context is gone. \
             Review your situation: \
             1. Check your team (workers under you) \
             2. Check your recent threads for any in-progress work \
             3. Check your workspace memory for saved state \
             4. Resume any interrupted work — do NOT re-hire or re-brief workers already in place \
             If everything is on track, respond with: [RECOVERY_OK] \
             If you need to resume something, briefly act on it.",
            restart_time
        ),
        _ => format!(
            "SYSTEM RESTART NOTICE: The system was restarted at {}. \
             Your container has been recovered but your in-memory context is gone. \
             Review your situation: \
             1. Check your workspace for any in-progress files or work \
             2. Check your recent threads for context on what you were working on \
             3. Check your workspace memory for saved state \
             4. Resume any interrupted work from where you left off \
             If everything is on track, respond with: [RECOVERY_OK] \
             If you have interrupted work to resume, briefly describe what you're picking back up.",
            restart_time
        ),
    };

    let instructions = "This is a system-generated restart notification. \
        Be concise. Review your state using available tools, then either resume \
        interrupted work or confirm everything is on track with [RECOVERY_OK]. \
        Do NOT narrate what you are about to do — just do it and respond with the result. \
        Do NOT repeat or summarize information you already know.";

    tracing::info!("Sending recovery prompt to {} ({})", agent_name, role);

    state.mark_agent_working(agent_id, "Post-restart recovery").await;
    let result = state.openclaw.send_message(agent_id, &prompt, Some(instructions)).await;
    state.mark_agent_done(agent_id).await;

    match result {
        Ok(response) => {
            let (cleaned, _) = strip_agent_tags(&response);
            let has_recovery_ok = {
                let normalized: String = response.chars()
                    .filter(|c| !c.is_whitespace() && *c != '[' && *c != ']')
                    .collect();
                normalized.contains("RECOVERY_OK") || normalized.contains("RECOVERYOK")
            };

            if has_recovery_ok {
                tracing::info!("Recovery: {} ({}) reports all clear", agent_name, role);
            } else if cleaned.trim().is_empty() {
                tracing::warn!("Recovery: {} ({}) returned empty response", agent_name, role);
            } else {
                tracing::info!("Recovery: {} ({}) is resuming work ({} chars)", agent_name, role, cleaned.len());

                // For MAIN agent only: post substantive recovery reports to user DM thread
                if role == "MAIN" {
                    let thread_id: Option<Uuid> = sqlx::query_scalar(
                        "SELECT tm.thread_id FROM thread_members tm \
                         JOIN threads t ON t.id = tm.thread_id \
                         JOIN thread_members tm2 ON t.id = tm2.thread_id \
                         WHERE tm.member_type = 'AGENT' AND tm.member_id = $1 \
                           AND tm2.member_type = 'USER' AND t.type = 'DM' LIMIT 1"
                    ).bind(agent_id).fetch_optional(&state.db).await.ok().flatten();

                    if let Some(tid) = thread_id {
                        let msg_id = Uuid::new_v4();
                        let prefixed = format!("[Post-Restart Recovery] {}", cleaned);
                        let content = json!({"text": prefixed});
                        if let Ok(agent_msg) = sqlx::query_as::<_, Message>(
                            "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) \
                             VALUES ($1,$2,'AGENT',$3,$4,0) \
                             RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
                        ).bind(msg_id).bind(tid).bind(agent_id).bind(&content)
                        .fetch_one(&state.db).await {
                            let _ = state.tx.send(json!({"type":"new_message","message": agent_msg}).to_string());
                        }
                    }
                }
            }
            Ok(())
        }
        Err(e) => {
            tracing::error!("Recovery prompt failed for {} ({}): {}", agent_name, role, e);
            Err(format!("Recovery prompt failed for {}: {}", agent_name, e))
        }
    }
}
