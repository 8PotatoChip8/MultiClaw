use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use uuid::Uuid;

use super::ws::{events_handler, AppState};
use crate::db::models::{Agent, Company, CreateCompanyRequest, Thread, Message, CreateMessageRequest};

// These are mock handlers for the MVP. In a full implementation, they would wire 
// to sqlx queries, the policy engine, and the dispatcher.

pub fn app_router(state: AppState) -> Router {
    Router::new()
        .route("/v1/install/init", post(handle_init))
        .route("/v1/companies", get(list_companies).post(create_company))
        .route("/v1/companies/:id/org-tree", get(get_org_tree))
        .route("/v1/agents/:id", get(get_agent))
        .route("/v1/companies/:id/hire-ceo", post(hire_ceo))
        .route("/v1/agents/:id/hire-manager", post(hire_manager))
        .route("/v1/agents/:id/hire-worker", post(hire_worker))
        .route("/v1/threads", get(get_threads))
        .route("/v1/threads/:id/messages", get(get_messages).post(send_message))
        .route("/v1/requests/:id/approve", post(approve_request))
        .route("/v1/requests/:id/reject", post(reject_request))
        .route("/v1/events", get(events_handler))
        .with_state(state)
}

async fn handle_init() -> impl IntoResponse {
    // 1. Create Holding Company
    // 2. Create MainAgent DB row
    // 3. Create tool_policies
    (StatusCode::OK, Json(json!({"status": "success"})))
}

async fn list_companies(State(state): State<AppState>) -> impl IntoResponse {
    let result = sqlx::query_as::<_, Company>(
        "SELECT id, holding_id, name, type, description, tags, status, created_at FROM companies"
    )
    .fetch_all(&state.db)
    .await;

    match result {
        Ok(companies) => (StatusCode::OK, Json(json!(companies))),
        Err(e) => {
            tracing::error!("Failed to fetch companies: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"})))
        }
    }
}

async fn create_company(
    State(state): State<AppState>,
    Json(payload): Json<CreateCompanyRequest>,
) -> impl IntoResponse {
    let id = Uuid::new_v4();
    // In a real system, holding_id comes from auth context. Using a dummy for now.
    let holding_id = Uuid::from_u128(0); 
    
    let result = sqlx::query_as::<_, Company>(
        r#"
        INSERT INTO companies (id, holding_id, name, type, description, status)
        VALUES ($1, $2, $3, $4, $5, 'ACTIVE')
        RETURNING id, holding_id, name, type, description, tags, status, created_at
        "#
    )
    .bind(id)
    .bind(holding_id)
    .bind(payload.name)
    .bind(payload.r#type)
    .bind(payload.description)
    .fetch_one(&state.db)
    .await;

    match result {
        Ok(company) => (StatusCode::CREATED, Json(json!(company))),
        Err(e) => {
            tracing::error!("Failed to create company: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"})))
        }
    }
}

async fn get_org_tree(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let company_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid ID"}))),
    };

    let result = sqlx::query_as::<_, Agent>(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, status, created_at FROM agents WHERE company_id = $1"
    )
    .bind(company_id)
    .fetch_all(&state.db)
    .await;

    match result {
        Ok(agents) => {
            // Very naive tree builder: Just dump list for now so UI has *something*
            (StatusCode::OK, Json(json!({"tree": agents})))
        },
        Err(e) => {
            tracing::error!("Failed to fetch org tree: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"})))
        }
    }
}

async fn get_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid ID"}))),
    };

    let result = sqlx::query_as::<_, Agent>(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, status, created_at FROM agents WHERE id = $1"
    )
    .bind(agent_id)
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some(agent)) => (StatusCode::OK, Json(json!(agent))),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "Agent not found"}))),
        Err(e) => {
            tracing::error!("Failed to fetch agent: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"})))
        }
    }
}

async fn hire_ceo(Path(_id): Path<String>) -> impl IntoResponse {
    // 1. Invoke Policy Engine `can_hire_second_ceo`
    // 2. If immediate, create DB rows and call `Provisioning::provision`
    (StatusCode::ACCEPTED, Json(json!({"status": "hiring_initiated"})))
}

async fn hire_manager(Path(_id): Path<String>) -> impl IntoResponse {
    (StatusCode::ACCEPTED, Json(json!({"status": "hiring_initiated"})))
}

async fn hire_worker(Path(_id): Path<String>) -> impl IntoResponse {
    (StatusCode::ACCEPTED, Json(json!({"status": "hiring_initiated"})))
}

async fn get_threads(State(state): State<AppState>) -> impl IntoResponse {
    let result = sqlx::query_as::<_, Thread>(
        "SELECT id, type, title, created_by_user_id, created_at FROM threads"
    )
    .fetch_all(&state.db)
    .await;

    match result {
        Ok(threads) => (StatusCode::OK, Json(json!(threads))),
        Err(e) => {
            tracing::error!("Failed to fetch threads: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"})))
        }
    }
}

async fn get_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let thread_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid ID"}))),
    };

    let result = sqlx::query_as::<_, Message>(
        "SELECT id, thread_id, sender_type, sender_id, content, created_at FROM messages WHERE thread_id = $1 ORDER BY created_at ASC"
    )
    .bind(thread_id)
    .fetch_all(&state.db)
    .await;

    match result {
        Ok(messages) => (StatusCode::OK, Json(json!(messages))),
        Err(e) => {
            tracing::error!("Failed to fetch messages: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"})))
        }
    }
}

async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<CreateMessageRequest>
) -> impl IntoResponse {
    let thread_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid ID"}))),
    };

    let message_id = Uuid::new_v4();

    let result = sqlx::query_as::<_, Message>(
        r#"
        INSERT INTO messages (id, thread_id, sender_type, sender_id, content) 
        VALUES ($1, $2, $3, $4, $5) 
        RETURNING id, thread_id, sender_type, sender_id, content, created_at
        "#
    )
    .bind(message_id)
    .bind(thread_id)
    .bind(payload.sender_type)
    .bind(payload.sender_id)
    .bind(payload.content)
    .fetch_one(&state.db)
    .await;

    match result {
        Ok(message) => {
            // 1. In true implementation, this would trigger the messaging Dispatcher actor here
            // to run the agent inference engine, sending events over the event broadcast channel.
            (StatusCode::CREATED, Json(json!(message)))
        },
        Err(e) => {
            tracing::error!("Failed to create message: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"})))
        }
    }
}

async fn approve_request(Path(_id): Path<String>) -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status": "approved"})))
}

async fn reject_request(Path(_id): Path<String>) -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status": "rejected"})))
}
