use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;
use rust_decimal::Decimal; // Requires rust_decimal crate for proper handling, but we map to NUMERIC. For MVP simplify to f64.

// In a real app we'd use rust_decimal. For this MVP we will just use f64 and strings.

pub struct Ledger {
    pool: PgPool,
}

impl Ledger {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn record_internal_transfer(
        &self,
        from_company_id: Uuid,
        to_company_id: Uuid,
        engagement_id: Uuid,
        amount: f64,
        memo: &str,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // Expense for from_company
        sqlx::query!(
            "INSERT INTO ledger_entries (id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual)
             VALUES ($1, $2, $3, $4, 'EXPENSE', $5, 'USD', $6, true)",
            Uuid::new_v4(),
            from_company_id,
            to_company_id,
            engagement_id,
            amount as f64, // sqlx mapped to Postgres NUMERIC requires BigDecimal/rust_decimal if you turn on features, but we didn't specify the rust_decimal feature. We will rely on sqlx casting or switch to string.
            memo
        )
        .execute(&mut *tx)
        .await?;

        // Revenue for to_company
        sqlx::query!(
            "INSERT INTO ledger_entries (id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual)
             VALUES ($1, $2, $3, $4, 'REVENUE', $5, 'USD', $6, true)",
            Uuid::new_v4(),
            to_company_id,
            from_company_id,
            engagement_id,
            amount as f64,
            memo
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}
