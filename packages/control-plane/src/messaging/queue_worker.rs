use crate::api::ws::AppState;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Notify;
use uuid::Uuid;

/// Represents a claimed queue item ready for processing.
#[derive(Debug, sqlx::FromRow)]
struct QueueItem {
    id: Uuid,
    agent_id: Uuid,
    kind: String,
    payload: serde_json::Value,
    retry_count: i16,
    max_retries: i16,
}

/// Reset any PROCESSING rows that have been stuck for more than 15 minutes.
/// This handles process crashes — the items get retried.
async fn recover_stale_claims(pool: &PgPool) {
    match sqlx::query(
        "UPDATE message_queue SET status = 'PENDING', claimed_at = NULL \
         WHERE status = 'PROCESSING' AND claimed_at < NOW() - INTERVAL '15 minutes'"
    )
    .execute(pool)
    .await
    {
        Ok(r) => {
            if r.rows_affected() > 0 {
                tracing::info!("[queue_worker] recovered {} stale PROCESSING items", r.rows_affected());
            }
        }
        Err(e) => tracing::error!("[queue_worker] failed to recover stale claims: {}", e),
    }
}

/// Claim up to N items from the queue — one per distinct agent, preferring
/// agents that don't already have a PROCESSING item. Ordered by priority ASC
/// (lower = higher priority), then created_at ASC (oldest first).
async fn claim_work(pool: &PgPool, limit: i64) -> Vec<QueueItem> {
    // Use nested CTEs: first lock candidate rows (FOR UPDATE requires no DISTINCT),
    // then pick one per agent_id with DISTINCT ON, then atomically mark PROCESSING.
    let items: Vec<QueueItem> = match sqlx::query_as::<_, QueueItem>(
        "WITH locked AS ( \
            SELECT id, agent_id, kind, payload, retry_count, max_retries, priority, created_at \
            FROM message_queue \
            WHERE status = 'PENDING' \
              AND agent_id NOT IN (SELECT agent_id FROM message_queue WHERE status = 'PROCESSING') \
            ORDER BY agent_id, priority ASC, created_at ASC \
            FOR UPDATE SKIP LOCKED \
        ), candidates AS ( \
            SELECT DISTINCT ON (agent_id) id, agent_id, kind, payload, retry_count, max_retries \
            FROM locked \
            ORDER BY agent_id, priority ASC, created_at ASC \
        ) \
        UPDATE message_queue q \
        SET status = 'PROCESSING', claimed_at = NOW() \
        FROM (SELECT id FROM candidates LIMIT $1) c \
        WHERE q.id = c.id \
        RETURNING q.id, q.agent_id, q.kind, q.payload, q.retry_count, q.max_retries"
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    {
        Ok(items) => items,
        Err(e) => {
            tracing::error!("[queue_worker] claim_work query failed: {}", e);
            vec![]
        }
    };

    items
}

/// Mark a queue item as completed.
async fn mark_completed(pool: &PgPool, item_id: Uuid) {
    let _ = sqlx::query(
        "UPDATE message_queue SET status = 'COMPLETED', completed_at = NOW() WHERE id = $1"
    )
    .bind(item_id)
    .execute(pool)
    .await;
}

/// Mark a queue item as failed (either retryable or permanent).
async fn mark_failed(pool: &PgPool, item_id: Uuid, error: &str, retry_count: i16, max_retries: i16) {
    if retry_count < max_retries {
        // Put back to PENDING for retry
        let _ = sqlx::query(
            "UPDATE message_queue SET status = 'PENDING', claimed_at = NULL, \
             retry_count = $1, error = $2 WHERE id = $3"
        )
        .bind(retry_count + 1)
        .bind(error)
        .bind(item_id)
        .execute(pool)
        .await;
        tracing::warn!("[queue_worker] item {} failed (retry {}/{}): {}", item_id, retry_count + 1, max_retries, error);
    } else {
        // Permanent failure
        let _ = sqlx::query(
            "UPDATE message_queue SET status = 'FAILED', completed_at = NOW(), error = $1 WHERE id = $2"
        )
        .bind(error)
        .bind(item_id)
        .execute(pool)
        .await;
        tracing::error!("[queue_worker] item {} permanently FAILED after {} retries: {}", item_id, max_retries, error);
    }
}

/// Process a single queue item by dispatching to the appropriate handler.
async fn process_item(state: &AppState, item: &QueueItem) -> Result<(), String> {
    match item.kind.as_str() {
        "thread_reply" => super::handlers::handle_thread_reply(state, &item.payload).await,
        "dm_initiate" => super::handlers::handle_dm_initiate(state, &item.payload).await,
        "dm_continue" => super::handlers::handle_dm_continue(state, &item.payload).await,
        "action_prompt" => super::handlers::handle_action_prompt(state, &item.payload).await,
        "heartbeat" => super::handlers::handle_heartbeat(state, &item.payload).await,
        "hire_notify" => super::handlers::handle_hire_notify(state, &item.payload).await,
        "approval_escalate" => super::handlers::handle_approval_escalate(state, &item.payload).await,
        "file_notify" => super::handlers::handle_file_notify(state, &item.payload).await,
        "recovery_prompt" => super::handlers::handle_recovery_prompt(state, &item.payload).await,
        "generic_send" => super::handlers::handle_generic_send(state, &item.payload).await,
        other => Err(format!("Unknown queue item kind: {}", other)),
    }
}

/// The main queue worker loop. Runs as a single background task.
pub async fn run(state: AppState, notify: Arc<Notify>) {
    tracing::info!("[queue_worker] started");

    // Recover any items that were PROCESSING when the server last crashed
    recover_stale_claims(&state.db).await;

    let mut stale_check_interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
    stale_check_interval.tick().await; // skip the first immediate tick

    loop {
        // Claim work — up to 10 items (one per agent)
        let items = claim_work(&state.db, 10).await;

        if items.is_empty() {
            // No work available — wait for notification or poll every 5s
            tokio::select! {
                _ = notify.notified() => {},
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(5)) => {},
                _ = stale_check_interval.tick() => {
                    recover_stale_claims(&state.db).await;
                    continue;
                },
            }
            continue;
        }

        // Fire-and-forget: spawn each item as an independent task.
        // Don't wait for tasks to finish — immediately loop back to claim_work.
        // claim_work's NOT IN (... WHERE status = 'PROCESSING') filter prevents
        // double-claiming for agents with in-flight work. When all agents with
        // pending work are busy, claim_work returns empty and we sleep above.
        for item in items {
            let state_clone = state.clone();
            let notify_clone = notify.clone();
            tokio::spawn(async move {
                let item_id = item.id;
                let agent_id = item.agent_id;
                let kind = item.kind.clone();

                tracing::info!(
                    "[queue_worker] processing {}:{} for agent {} (retry {}/{})",
                    kind, item_id, agent_id, item.retry_count, item.max_retries
                );

                // Acquire the per-agent turn lock to serialize messages
                let _agent_guard = state_clone.acquire_agent_turn(agent_id).await;

                match process_item(&state_clone, &item).await {
                    Ok(()) => {
                        mark_completed(&state_clone.db, item_id).await;
                        tracing::info!("[queue_worker] completed {}:{}", kind, item_id);
                    }
                    Err(e) => {
                        mark_failed(&state_clone.db, item_id, &e, item.retry_count, item.max_retries).await;
                    }
                }

                // Wake queue worker to immediately claim this agent's next
                // pending item (if any), without waiting for the 5s poll.
                notify_clone.notify_one();
            });
        }

    }
}
