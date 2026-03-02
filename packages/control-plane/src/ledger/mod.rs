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
        sqlx::query(
            "INSERT INTO ledger_entries (id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual)
             VALUES ($1, $2, $3, $4, 'EXPENSE', $5, 'USD', $6, true)"
        )
        .bind(Uuid::new_v4())
        .bind(from_company_id)
        .bind(to_company_id)
        .bind(engagement_id)
        .bind(amount as f64)
        .bind(memo)
        .execute(&mut *tx)
        .await?;

        // Revenue for to_company
        sqlx::query(
            "INSERT INTO ledger_entries (id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual)
             VALUES ($1, $2, $3, $4, 'REVENUE', $5, 'USD', $6, true)"
        )
        .bind(Uuid::new_v4())
        .bind(to_company_id)
        .bind(from_company_id)
        .bind(engagement_id)
        .bind(amount as f64)
        .bind(memo)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}
