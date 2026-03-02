use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;
use serde_json::Value;

pub struct Engagements {
    pool: PgPool,
}

impl Engagements {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create_engagement(
        &self,
        service_id: Uuid,
        client_company_id: Uuid,
        provider_company_id: Uuid,
        scope: Value,
        created_by_agent_id: Uuid,
    ) -> Result<Uuid> {
        // 1. Create a shared thread for this engagement
        let thread_id = Uuid::new_v4();
        sqlx::query!(
            "INSERT INTO threads (id, type, title) VALUES ($1, 'ENGAGEMENT', 'Service Engagement')",
            thread_id
        ).execute(&self.pool).await?;

        // 2. Add client company to thread
        sqlx::query!(
            "INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'COMPANY', $2)",
            thread_id, client_company_id
        ).execute(&self.pool).await?;

        // 3. Add provider company to thread
        sqlx::query!(
            "INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'COMPANY', $2)",
            thread_id, provider_company_id
        ).execute(&self.pool).await?;

        // 4. Create the engagement record
        let engagement_id = Uuid::new_v4();
        sqlx::query!(
            "INSERT INTO service_engagements (id, service_id, client_company_id, provider_company_id, scope, status, created_by_agent_id, thread_id)
             VALUES ($1, $2, $3, $4, $5, 'ACTIVE', $6, $7)",
            engagement_id,
            service_id,
            client_company_id,
            provider_company_id,
            scope,
            created_by_agent_id,
            thread_id
        ).execute(&self.pool).await?;

        Ok(engagement_id)
    }
}
