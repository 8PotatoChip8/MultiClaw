use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use sqlx::types::JsonValue;

// ─── Holdings ──────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Holding {
    pub id: Uuid,
    pub owner_user_id: Option<Uuid>,
    pub name: String,
    pub main_agent_name: String,
    pub created_at: DateTime<Utc>,
}

// ─── Companies ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Company {
    pub id: Uuid,
    pub holding_id: Uuid,
    pub name: String,
    pub r#type: String,
    pub description: Option<String>,
    pub tags: Option<JsonValue>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateCompanyRequest {
    pub name: String,
    pub r#type: String,
    pub description: Option<String>,
}

// ─── Tool Policies ─────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ToolPolicy {
    pub id: Uuid,
    pub name: String,
    pub allowlist: JsonValue,
    pub denylist: JsonValue,
    pub notes: Option<String>,
}

// ─── Agents ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Agent {
    pub id: Uuid,
    pub holding_id: Uuid,
    pub company_id: Option<Uuid>,
    pub role: String,
    pub name: String,
    pub specialty: Option<String>,
    pub parent_agent_id: Option<Uuid>,
    pub preferred_model: Option<String>,
    pub effective_model: String,
    pub system_prompt: Option<String>,
    pub tool_policy_id: Uuid,
    pub vm_id: Option<Uuid>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAgentRequest {
    pub company_id: Uuid,
    pub role: String,
    pub name: String,
    pub specialty: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PatchAgentRequest {
    pub preferred_model: Option<String>,
    pub specialty: Option<String>,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HireRequest {
    pub name: String,
    pub specialty: Option<String>,
    pub preferred_model: Option<String>,
}

// ─── VMs ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Vm {
    pub id: Uuid,
    pub provider: String,
    pub provider_ref: String,
    pub hostname: String,
    pub ip_address: Option<String>,
    pub resources: JsonValue,
    pub state: String,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

// ─── Threads ───────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Thread {
    pub id: Uuid,
    pub r#type: String,
    pub title: Option<String>,
    pub created_by_user_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateThreadRequest {
    pub r#type: String,
    pub title: Option<String>,
    pub member_ids: Option<Vec<Uuid>>,
}

// ─── Messages ──────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Message {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub sender_type: String,
    pub sender_id: Uuid,
    pub content: JsonValue,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateMessageRequest {
    pub sender_type: Option<String>,
    pub sender_id: Option<Uuid>,
    pub content: JsonValue,
    pub targets: Option<Vec<Uuid>>,
    pub mode: Option<String>,
}

// ─── Dispatches ────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Dispatch {
    pub id: Uuid,
    pub message_id: Uuid,
    pub target_agent_id: Uuid,
    pub mode: String,
    pub status: String,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

// ─── Requests & Approvals ──────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Request {
    pub id: Uuid,
    pub r#type: String,
    pub created_by_agent_id: Option<Uuid>,
    pub created_by_user_id: Option<Uuid>,
    pub company_id: Option<Uuid>,
    pub payload: JsonValue,
    pub status: String,
    pub current_approver_type: String,
    pub current_approver_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRequestPayload {
    pub r#type: String,
    pub company_id: Option<Uuid>,
    pub payload: JsonValue,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Approval {
    pub id: Uuid,
    pub request_id: Uuid,
    pub approver_type: String,
    pub approver_id: Uuid,
    pub decision: String,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ApprovalAction {
    pub note: Option<String>,
}

// ─── Service Catalog ───────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ServiceCatalogItem {
    pub id: Uuid,
    pub provider_company_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub pricing_model: String,
    pub rate: JsonValue,
    pub tags: Option<JsonValue>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateServiceRequest {
    pub provider_company_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub pricing_model: String,
    pub rate: JsonValue,
}

// ─── Service Engagements ───────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ServiceEngagement {
    pub id: Uuid,
    pub service_id: Uuid,
    pub client_company_id: Uuid,
    pub provider_company_id: Uuid,
    pub scope: JsonValue,
    pub status: String,
    pub created_by_agent_id: Option<Uuid>,
    pub thread_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEngagementRequest {
    pub service_id: Uuid,
    pub client_company_id: Uuid,
    pub scope: JsonValue,
    pub created_by_agent_id: Option<Uuid>,
}

// ─── Ledger ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct LedgerEntry {
    pub id: Uuid,
    pub company_id: Uuid,
    pub counterparty_company_id: Option<Uuid>,
    pub engagement_id: Option<Uuid>,
    pub r#type: String,
    pub amount: rust_decimal::Decimal,
    pub currency: String,
    pub memo: Option<String>,
    pub is_virtual: bool,
    pub created_at: DateTime<Utc>,
}

// ─── Secrets ───────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Secret {
    pub id: Uuid,
    pub scope_type: String,
    pub scope_id: Uuid,
    pub kind: String,
    pub ciphertext: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

// ─── Install Init ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct InitRequest {
    pub holding_name: Option<String>,
    pub main_agent_name: Option<String>,
    pub default_model: Option<String>,
    pub strict_mode: Option<bool>,
    pub vm_provider: Option<String>,
}
