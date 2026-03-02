use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

// The dispatcher handles routing a new message to the relevant agent VMs via their WebSocket,
// or via HTTP for simple operations. In MultiClaw, messages belong to threads.

#[derive(Debug)]
pub enum DispatchMode {
    Parallel,
    Sequential,
}

pub struct Dispatcher {
    pool: PgPool,
}

impl Dispatcher {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Creates a dispatch record and enqueues the message to be sent to target agent(s).
    pub async fn dispatch_message(
        &self,
        message_id: Uuid,
        target_agent_ids: Vec<Uuid>,
        mode: DispatchMode,
    ) -> Result<()> {
        let mode_str = match mode {
            DispatchMode::Parallel => "PARALLEL",
            DispatchMode::Sequential => "SEQUENTIAL",
        };

        for &agent_id in &target_agent_ids {
            sqlx::query!(
                "INSERT INTO dispatches (id, message_id, target_agent_id, mode, status)
                 VALUES ($1, $2, $3, $4, 'PENDING')",
                Uuid::new_v4(),
                message_id,
                agent_id,
                mode_str,
            )
            .execute(&self.pool)
            .await?;
        }

        // Ideally, we signal a background worker (e.g. via tokio channel) that there are pending
        // dispatches. The background worker picks them up and pushes them to the Agent VM WebSockets.
        // For MVP, if we connect to an open websocket mapped by agent_id, we send immediately.

        Ok(())
    }

    /// Updates the dispatch status (e.g. when an agent finishes streaming its response).
    pub async fn complete_dispatch(&self, dispatch_id: Uuid, error: Option<String>) -> Result<()> {
        let status = if error.is_some() { "ERROR" } else { "COMPLETED" };
        
        sqlx::query!(
            "UPDATE dispatches SET status = $1, error = $2, completed_at = NOW() WHERE id = $3",
            status,
            error,
            dispatch_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
