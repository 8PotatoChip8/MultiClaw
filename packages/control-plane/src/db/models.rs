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
    pub sandbox_vm_id: Option<Uuid>,
    pub handle: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
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

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Vm {
    pub id: Uuid,
    pub provider: String,
    pub provider_ref: String,
    pub hostname: String,
    pub ip_address: Option<String>,
    pub resources: JsonValue,
    pub state: String,
    pub vm_type: String,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

// ─── Shared VMs ───────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct SharedVm {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub scope_type: String,
    pub company_id: Uuid,
    pub department_manager_id: Option<Uuid>,
    pub vm_purpose: String,
    pub provisioned_by_agent_id: Uuid,
    pub label: Option<String>,
    pub resource_limits: JsonValue,
    pub created_at: DateTime<Utc>,
}

/// Joined view returned by list/get endpoints.
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct SharedVmDetail {
    // shared_vms fields
    pub id: Uuid,
    pub vm_id: Uuid,
    pub scope_type: String,
    pub company_id: Uuid,
    pub department_manager_id: Option<Uuid>,
    pub vm_purpose: String,
    pub provisioned_by_agent_id: Uuid,
    pub label: Option<String>,
    pub resource_limits: JsonValue,
    pub created_at: DateTime<Utc>,
    // joined from vms
    pub provider_ref: String,
    pub hostname: String,
    pub ip_address: Option<String>,
    pub resources: JsonValue,
    pub state: String,
}

#[derive(Debug, Deserialize)]
pub struct ProvisionSharedVmRequest {
    pub requester_agent_id: Uuid,
    pub company_id: Uuid,
    pub vm_purpose: String,
    pub department_manager_id: Option<Uuid>,
    pub label: Option<String>,
    pub resources: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
pub struct SharedVmExecRequest {
    pub agent_id: Uuid,
    pub command: String,
    pub user: Option<String>,
    pub working_dir: Option<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct SharedVmFileRequest {
    pub agent_id: Uuid,
    pub path: String,
    pub content: Option<String>,
    pub encoding: Option<String>,
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
    pub reply_depth: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateMessageRequest {
    pub sender_type: Option<String>,
    pub sender_id: Option<Uuid>,
    pub content: JsonValue,
    pub reply_depth: Option<i32>,
}

// ─── Dispatches ────────────────────────────────────────────────

#[allow(dead_code)]
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
    pub requester_id: Uuid,
    pub company_id: Option<Uuid>,
    pub payload: JsonValue,
}

#[derive(Debug, Deserialize)]
pub struct AgentApprovalAction {
    pub agent_id: Uuid,
    pub note: Option<String>,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

// ─── Meetings ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Meeting {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub topic: String,
    pub organizer_id: Uuid,
    pub status: String,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub summary: Option<String>,
    pub closed_at: Option<DateTime<Utc>>,
    pub closed_by_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateMeetingRequest {
    pub topic: String,
    pub organizer_id: Uuid,
    pub participant_ids: Vec<Uuid>,
    pub scheduled_for: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct CloseMeetingRequest {
    pub closed_by_id: Uuid,
}

// ─── Ledger ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct LedgerEntry {
    pub id: Uuid,
    pub company_id: Uuid,
    pub counterparty_company_id: Option<Uuid>,
    pub engagement_id: Option<Uuid>,
    pub order_id: Option<Uuid>,
    pub r#type: String,
    pub amount: rust_decimal::Decimal,
    pub currency: String,
    pub memo: Option<String>,
    pub is_virtual: bool,
    pub created_at: DateTime<Utc>,
}

// ─── Trading Orders ───────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct TradingOrder {
    pub id: Uuid,
    pub company_id: Uuid,
    pub agent_id: Uuid,
    pub exchange: String,
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub quantity: rust_decimal::Decimal,
    pub price: Option<rust_decimal::Decimal>,
    pub quote_currency: String,
    pub status: String,
    pub exchange_order_id: Option<String>,
    pub fill_price: Option<rust_decimal::Decimal>,
    pub fill_quantity: Option<rust_decimal::Decimal>,
    pub fee: Option<rust_decimal::Decimal>,
    pub fee_currency: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub filled_at: Option<DateTime<Utc>>,
}

// ─── Secrets ───────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Secret {
    pub id: Uuid,
    pub scope_type: String,
    pub scope_id: Uuid,
    pub kind: String,
    pub ciphertext: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

// ─── File Transfers ─────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct FileTransfer {
    pub id: Uuid,
    pub sender_id: Uuid,
    pub receiver_id: Uuid,
    pub filename: String,
    pub size_bytes: i64,
    pub encoding: String,
    pub dest_path: String,
    pub status: String,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AgentFileSendRequest {
    pub target: String,
    pub src_path: String,
    pub dest_path: Option<String>,
    pub encoding: Option<String>,
}

// ─── Install Init ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct InitRequest {
    pub holding_name: Option<String>,
    pub main_agent_name: Option<String>,
    pub default_model: Option<String>,
}
