use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use sqlx::types::JsonValue;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Company {
    pub id: Uuid,
    pub holding_id: Uuid,
    pub name: String,
    pub r#type: String, // 'INTERNAL' or 'EXTERNAL'
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

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Thread {
    pub id: Uuid,
    pub r#type: String,
    pub title: Option<String>,
    pub created_by_user_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Message {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub sender_type: String, // 'USER' | 'AGENT' | 'SYSTEM'
    pub sender_id: Uuid,
    pub content: JsonValue,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateMessageRequest {
    pub sender_type: String,
    pub sender_id: Uuid,
    pub content: JsonValue,
}
