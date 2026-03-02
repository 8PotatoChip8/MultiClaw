use anyhow::Result;
use sqlx::PgPool;
use serde_json::Value;
use uuid::Uuid;

pub struct ServiceCatalog {
    pool: PgPool,
}

impl ServiceCatalog {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create_service(
        &self,
        provider_company_id: Uuid,
        name: &str,
        description: &str,
        pricing_model: &str,
        rate: Value,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        sqlx::query!(
            "INSERT INTO service_catalog (id, provider_company_id, name, description, pricing_model, rate)
             VALUES ($1, $2, $3, $4, $5, $6)",
            id,
            provider_company_id,
            name,
            description,
            pricing_model,
            rate
        )
        .execute(&self.pool)
        .await?;

        Ok(id)
    }

    pub async fn list_services(&self) -> Result<Vec<serde_json::Value>> {
        // MVP: return empty or query json
        Ok(vec![])
    }
}
